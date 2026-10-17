//! Interpreter filesystem port backed by the verified recovery protocol.
use super::*;
use crate::recovery::paths;
use interpreter::runtime::filesystem::{
    DirectoryEntry, DirectoryReader, FileReader, FileWriter, Filesystem,
    FilesystemError as PortError, FilesystemInfo, Invocation, ReaderOptions, ResourceDescriptor,
    ResourceKind,
};
use interpreter::runtime::{fs, vfs::Placement};

#[async_trait::async_trait]
impl Filesystem for DurableFilesystem {
    fn info(&self) -> FilesystemInfo {
        FilesystemInfo::from_vfs(self.workspace.vfs())
    }
    fn cwd(&self) -> &str {
        self.workspace.vfs().cwd()
    }
    fn path_cost(&self, path: &str) -> u64 {
        use interpreter::runtime::fuel;
        fuel::SCAN
            .cost(path.len() as u64)
            .saturating_add(fuel::SYSCALL.cost(path.split('/').count() as u64))
    }
    fn placement(&self, path: &str) -> Result<Placement, PortError> {
        Ok(
            paths::resolve_content(self.workspace.vfs(), self.cwd(), path)?
                .placement()
                .clone(),
        )
    }
    async fn metadata(
        &self,
        _invocation: &Invocation,
        path: &str,
        follow: bool,
    ) -> Result<cap_std::fs::Metadata, PortError> {
        let vfs = self.workspace.vfs().clone();
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            if follow {
                paths::resolve_content(&vfs, vfs.cwd(), &path)?.metadata()
            } else {
                paths::check_access(&vfs, vfs.cwd(), &path)?;
                fs::resolve_link(&vfs, vfs.cwd(), &path)?.symlink_metadata()
            }
        })
        .await
        .map_err(|error| PortError::Backend(Box::new(error)))?
        .map_err(Into::into)
    }
    async fn read(
        &self,
        _invocation: &Invocation,
        path: &str,
        offset: u64,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, PortError> {
        let vfs = self.workspace.vfs().clone();
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || {
            let file = paths::resolve_content(&vfs, vfs.cwd(), &path)?.open()?;
            interpreter::runtime::filesystem::bounded_read(file, offset, maximum_bytes)
        })
        .await
        .map_err(|error| PortError::Backend(Box::new(error)))?
    }
    async fn write(
        &self,
        invocation: &Invocation,
        path: &str,
        bytes: Vec<u8>,
        append: bool,
    ) -> Result<(), PortError> {
        let filesystem = self.metered(invocation);
        let id = operation(invocation)?;
        filesystem
            .charge(interpreter::runtime::fuel::HASH, bytes.len() as u64)
            .map_err(backend)?;
        let input =
            ReplayInput::bytes(id.to_owned(), bytes).map_err(|error| backend(error.into()))?;
        let content = input.content();
        let request = if append {
            Mutation::Append {
                path: path.into(),
                content,
            }
        } else {
            Mutation::Write {
                path: path.into(),
                content,
            }
        };
        filesystem
            .execute_with_input(id, request, input)
            .await
            .map_err(backend)?;
        Ok(())
    }
    async fn mkdir(
        &self,
        invocation: &Invocation,
        path: &str,
        recursive: bool,
    ) -> Result<(), PortError> {
        let filesystem = self.metered(invocation);
        let id = operation(invocation)?;
        if recursive {
            return Err(PortError::Unsupported(
                "recursive directory creation requires caller-orchestrated primitive operations",
            ));
        }
        filesystem
            .execute(id, Mutation::CreateDirectory { path: path.into() })
            .await
            .map_err(backend)?;
        Ok(())
    }
    async fn remove(
        &self,
        invocation: &Invocation,
        path: &str,
        recursive: bool,
        budget: &mut dyn interpreter::runtime::filesystem::MutationBudget,
    ) -> Result<(), PortError> {
        let filesystem = self.metered(invocation);
        let id = operation(invocation)?;
        if recursive {
            return Err(PortError::Unsupported(
                "recursive removal requires caller-orchestrated primitive operations",
            ));
        }
        filesystem
            .execute(id, Mutation::Remove { path: path.into() })
            .await
            .map_err(backend)?;
        budget.effect();
        Ok(())
    }
    async fn move_entry(
        &self,
        invocation: &Invocation,
        from: &str,
        to: &str,
        budget: &mut dyn interpreter::runtime::filesystem::MutationBudget,
    ) -> Result<(), PortError> {
        let filesystem = self.metered(invocation);
        let id = operation(invocation)?;
        filesystem
            .execute(
                id,
                Mutation::Move {
                    source: from.into(),
                    destination: to.into(),
                },
            )
            .await
            .map_err(backend)?;
        budget.effect();
        Ok(())
    }
    async fn copy(
        &self,
        _: &Invocation,
        _: &str,
        _: &str,
        _: bool,
        _: &mut dyn interpreter::runtime::filesystem::MutationBudget,
    ) -> Result<(), PortError> {
        Err(PortError::Unsupported(
            "copy requires caller-supplied reads and writes",
        ))
    }
    async fn open_reader(
        &self,
        invocation: &Invocation,
        path: &str,
        options: ReaderOptions,
    ) -> Result<Box<dyn FileReader>, PortError> {
        let filesystem = self.metered(invocation);
        Ok(Box::new(
            DurableFilesystem::open_reader(&filesystem, operation(invocation)?, path, options)
                .await
                .map_err(backend)?,
        ))
    }
    async fn open_directory(
        &self,
        invocation: &Invocation,
        path: &str,
        recursive: bool,
        limits: &interpreter::runtime::limits::TenantLimits,
    ) -> Result<Box<dyn DirectoryReader>, PortError> {
        // Enumeration has a fixed 4 MiB encoded ceiling. Admit its worst-case
        // working set before SQL decoding or native enumeration. The guard also
        // covers codec/validation copies until the snapshot has been committed.
        let memory = Arc::new(
            interpreter::runtime::limits::HostBytes::new(
                limits,
                (crate::recovery::MAX_LISTING_BYTES as u64) * 8 + 8192,
            )
            .map_err(|error| PortError::Execution(error.into()))?,
        );
        let filesystem = self.metered(invocation);
        let id = operation(invocation)?;
        let resource = id.to_owned();
        let path = path.to_owned();
        finish_owned(async move {
            let _memory = memory.clone();
            filesystem
                .open_directory_reserved(&resource, &path, recursive, 0, Some(memory))
                .await
                .map(|_| ())
        })
        .await
        .map_err(backend)?;
        Ok(Box::new(DurableDirectory {
            filesystem: self.clone(),
            resource: id.into(),
        }))
    }
    fn restore_reader(
        &self,
        descriptor: ResourceDescriptor,
    ) -> Result<Box<dyn FileReader>, PortError> {
        check_descriptor(self, &descriptor)?;
        let ResourceKind::Reader(options) = descriptor.kind else {
            return Err(backend(RecoveryError::Conflict.into()));
        };
        Ok(Box::new(FilesystemReader {
            filesystem: self.clone(),
            resource: descriptor.id,
            options,
        }))
    }
    fn restore_directory(
        &self,
        descriptor: ResourceDescriptor,
    ) -> Result<Box<dyn DirectoryReader>, PortError> {
        check_descriptor(self, &descriptor)?;
        if descriptor.kind != ResourceKind::Directory {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        Ok(Box::new(DurableDirectory {
            filesystem: self.clone(),
            resource: descriptor.id,
        }))
    }
    fn restore_writer(
        &self,
        descriptor: ResourceDescriptor,
    ) -> Result<Box<dyn FileWriter>, PortError> {
        check_descriptor(self, &descriptor)?;
        if descriptor.kind != ResourceKind::Writer {
            return Err(backend(RecoveryError::Conflict.into()));
        }
        Ok(Box::new(DurableWriter {
            filesystem: self.clone(),
            resource: descriptor.id,
            closed: false,
        }))
    }
    async fn open_writer(
        &self,
        invocation: &Invocation,
        path: &str,
    ) -> Result<Box<dyn FileWriter>, PortError> {
        let id = operation(invocation)?;
        DurableFilesystem::open_writer(self, id, path)
            .await
            .map_err(backend)?;
        Ok(Box::new(DurableWriter {
            filesystem: self.clone(),
            resource: id.into(),
            closed: false,
        }))
    }
}
struct DurableWriter {
    filesystem: DurableFilesystem,
    resource: String,
    closed: bool,
}
#[async_trait::async_trait]
impl FileWriter for DurableWriter {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        Some(ResourceDescriptor {
            scope: self.filesystem.id.clone(),
            id: self.resource.clone(),
            kind: ResourceKind::Writer,
        })
    }
    fn is_closed(&self) -> bool {
        self.closed
    }
    async fn write(&mut self, invocation: &Invocation, bytes: Vec<u8>) -> Result<(), PortError> {
        let id = operation(invocation)?;
        let filesystem = self.filesystem.metered(invocation);
        filesystem
            .write_chunk(&self.resource, id, bytes)
            .await
            .map_err(backend)
    }
    async fn close(&mut self, invocation: &Invocation) -> Result<(), PortError> {
        let id = operation(invocation)?;
        let filesystem = self.filesystem.metered(invocation);
        let result = filesystem.close_writer(&self.resource, id).await;
        // SQL remains authoritative after either a successful close or a saved
        // refusal, and after an acknowledgement is lost.
        self.closed = filesystem
            .journal
            .writer_is_closed(&filesystem.id, &self.resource)
            .await
            .map_err(backend)?;
        result.map(|_| ()).map_err(backend)
    }
}
pub(super) fn operation(invocation: &Invocation) -> Result<&str, PortError> {
    invocation.operation.as_deref().ok_or_else(|| {
        PortError::Backend(Box::new(FilesystemError::Configuration(
            "durable filesystem operation requires a stable operation identity",
        )))
    })
}
pub(super) fn backend(error: FilesystemError) -> PortError {
    match error {
        FilesystemError::Rejected(reason) => PortError::Rejected(reason),
        FilesystemError::Memory(error) => PortError::Execution(error.into()),
        FilesystemError::Recovery(RecoveryError::Fuel(error)) => PortError::Exhausted(error),
        other => PortError::Backend(Box::new(other)),
    }
}

fn check_descriptor(
    filesystem: &DurableFilesystem,
    descriptor: &ResourceDescriptor,
) -> Result<(), PortError> {
    validate_id(&descriptor.id).map_err(backend)?;
    if descriptor.scope != filesystem.id {
        return Err(backend(RecoveryError::Conflict.into()));
    }
    Ok(())
}
struct DurableDirectory {
    filesystem: DurableFilesystem,
    resource: String,
}
#[async_trait::async_trait]
impl DirectoryReader for DurableDirectory {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        Some(ResourceDescriptor {
            scope: self.filesystem.id.clone(),
            id: self.resource.clone(),
            kind: ResourceKind::Directory,
        })
    }
    async fn next_entry(
        &mut self,
        invocation: &Invocation,
        limits: &interpreter::runtime::limits::TenantLimits,
    ) -> Result<Option<DirectoryEntry>, PortError> {
        self.filesystem
            .metered(invocation)
            .listing_step(&self.resource, operation(invocation)?, false, limits)
            .await
            .map_err(backend)
    }
    fn retained_bytes(&self) -> u64 {
        0
    }
    async fn close(
        &mut self,
        invocation: &Invocation,
        limits: &interpreter::runtime::limits::TenantLimits,
    ) -> Result<(), PortError> {
        self.filesystem
            .metered(invocation)
            .listing_step(&self.resource, operation(invocation)?, true, limits)
            .await
            .map(|_| ())
            .map_err(backend)
    }
}
