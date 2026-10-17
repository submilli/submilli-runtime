//! Interpreter filesystem port backed by the verified recovery protocol.
use super::*;
use crate::recovery::paths;
use interpreter::runtime::filesystem::{
    DirectoryEntry, DirectoryReader, FileReader, FileWriter, Filesystem,
    FilesystemError as PortError, FilesystemInfo, Invocation,
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
    async fn metadata(&self, path: &str, follow: bool) -> Result<cap_std::fs::Metadata, PortError> {
        let vfs = self.workspace.vfs().clone();
        let path = path.to_owned();
        let access = self.context.filesystem_resources.clone().read_owned().await;
        tokio::task::spawn_blocking(move || {
            let _access = access;
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
        path: &str,
        offset: u64,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, PortError> {
        let vfs = self.workspace.vfs().clone();
        let path = path.to_owned();
        let access = self.context.filesystem_resources.clone().read_owned().await;
        tokio::task::spawn_blocking(move || {
            let _access = access;
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
                "recursive directory creation requires a prepared plan",
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
                "recursive removal requires a prepared plan",
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
            "copy requires captured sources and a prepared plan",
        ))
    }
    async fn open_reader(
        &self,
        invocation: &Invocation,
        path: &str,
    ) -> Result<Box<dyn FileReader>, PortError> {
        let filesystem = self.metered(invocation);
        Ok(Box::new(
            DurableFilesystem::open_reader(&filesystem, operation(invocation)?, path)
                .await
                .map_err(backend)?,
        ))
    }
    async fn open_directory(
        &self,
        invocation: &Invocation,
        path: &str,
        recursive: bool,
    ) -> Result<Box<dyn DirectoryReader>, PortError> {
        let filesystem = self.metered(invocation);
        let snapshot = DurableFilesystem::open_directory(
            &filesystem,
            operation(invocation)?,
            path,
            recursive,
            0,
        )
        .await
        .map_err(backend)?;
        if let Some(work) = &invocation.work {
            work.charge_fuel(interpreter::runtime::fuel::sort_cost(
                snapshot.remaining_entries() as u64,
            ))?;
        }
        Ok(Box::new(DurableDirectory(Some(snapshot.depth_first()))))
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
    fn is_closed(&self) -> bool {
        self.closed
    }
    async fn write(&mut self, invocation: &Invocation, bytes: Vec<u8>) -> Result<(), PortError> {
        let id = operation(invocation)?;
        let filesystem = self.filesystem.metered(invocation);
        if self.closed {
            return Err(PortError::Unsupported("writer is closed"));
        }
        filesystem
            .write_chunk(&self.resource, id, bytes)
            .await
            .map_err(backend)
    }
    async fn close(&mut self, invocation: &Invocation) -> Result<(), PortError> {
        let id = operation(invocation)?;
        let filesystem = self.filesystem.metered(invocation);
        filesystem
            .close_writer(&self.resource, id)
            .await
            .map_err(backend)?;
        self.closed = true;
        Ok(())
    }
}
fn operation(invocation: &Invocation) -> Result<&str, PortError> {
    invocation.operation.as_deref().ok_or_else(|| {
        PortError::Backend(Box::new(FilesystemError::Configuration(
            "durable filesystem operation requires a stable operation identity",
        )))
    })
}
fn backend(error: FilesystemError) -> PortError {
    match error {
        FilesystemError::Rejected(reason) => PortError::Rejected(reason),
        FilesystemError::Recovery(RecoveryError::Fuel(error)) => PortError::Exhausted(error),
        other => PortError::Backend(Box::new(other)),
    }
}

struct DurableDirectory(Option<DirectorySnapshot>);
impl DirectoryReader for DurableDirectory {
    fn next_entry(&mut self) -> Option<DirectoryEntry> {
        self.0.as_mut()?.next().map(|entry| DirectoryEntry {
            name: entry.path.rsplit('/').next().unwrap_or_default().into(),
            path: entry.path,
            kind: if entry.directory {
                "directory"
            } else if entry.symlink {
                "symlink"
            } else {
                "file"
            },
            bytes: if entry.directory || entry.symlink {
                0
            } else {
                entry.bytes
            },
        })
    }
    fn retained_bytes(&self) -> u64 {
        self.0.as_ref().map_or(0, DirectorySnapshot::retained_bytes)
    }
    fn close(&mut self) {
        self.0 = None;
    }
}
