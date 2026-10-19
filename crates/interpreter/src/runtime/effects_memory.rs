//! Check the encoded shape before Serde allocates collections or scratch buffers.
//! A native outcome declares the largest element its sequence deserializer owns.
//! Strings and binary payloads are counted by length, rather than multiplying an
//! entire response by a worst-case collection expansion factor.
use super::host::fatal_host_error;

pub(super) fn decoded_heap_bound(
    bytes: &[u8],
    element_bytes: usize,
    dynamic_maps: bool,
) -> wasmtime::Result<u64> {
    let mut input = Input {
        bytes,
        position: 0,
        heap: 128,
        element_bytes,
        dynamic_maps,
    };
    input.value(0)?;
    if input.position != bytes.len() {
        return Err(fatal_host_error("trailing native outcome bytes"));
    }
    Ok(input.heap)
}

struct Input<'a> {
    bytes: &'a [u8],
    position: usize,
    heap: u64,
    element_bytes: usize,
    dynamic_maps: bool,
}
impl Input<'_> {
    fn value(&mut self, depth: usize) -> wasmtime::Result<()> {
        if depth >= 128 {
            return Err(fatal_host_error("native outcome nesting exceeds limit"));
        }
        let marker = self.integer(1)? as u8;
        match marker {
            0x00..=0x7f | 0xe0..=0xff | 0xc0 | 0xc2 | 0xc3 => Ok(()),
            0xa0..=0xbf => self.payload(u64::from(marker & 0x1f)),
            0x90..=0x9f => self.sequence(u64::from(marker & 0x0f), depth),
            0x80..=0x8f => self.map(u64::from(marker & 0x0f), depth),
            0xcc | 0xd0 => self.skip(1),
            0xcd | 0xd1 => self.skip(2),
            0xce | 0xd2 | 0xca => self.skip(4),
            0xcf | 0xd3 | 0xcb => self.skip(8),
            0xc4 | 0xd9 => {
                let length = self.integer(1)?;
                self.payload(length)
            }
            0xc5 | 0xda => {
                let length = self.integer(2)?;
                self.payload(length)
            }
            0xc6 | 0xdb => {
                let length = self.integer(4)?;
                self.payload(length)
            }
            0xdc => {
                let count = self.integer(2)?;
                self.sequence(count, depth)
            }
            0xdd => {
                let count = self.integer(4)?;
                self.sequence(count, depth)
            }
            0xde => {
                let count = self.integer(2)?;
                self.map(count, depth)
            }
            0xdf => {
                let count = self.integer(4)?;
                self.map(count, depth)
            }
            _ => Err(fatal_host_error("unsupported native outcome marker")),
        }
    }
    fn sequence(&mut self, count: u64, depth: usize) -> wasmtime::Result<()> {
        if count > self.remaining() as u64 {
            return Err(fatal_host_error("truncated native outcome sequence"));
        }
        if self.element_bytes == 0 {
            return Err(fatal_host_error(
                "native outcome codec has no sequence allocation bound",
            ));
        }
        // Vec's geometric growth is at most twice its length, with a minimum
        // allocation of eight elements. No untrusted size hint reaches Serde
        // until the complete encoded collection has been checked here.
        self.add(
            count
                .saturating_mul(2)
                .max(8)
                .saturating_mul(self.element_bytes as u64),
        )?;
        for _ in 0..count {
            self.value(depth + 1)?;
        }
        Ok(())
    }
    fn map(&mut self, count: u64, depth: usize) -> wasmtime::Result<()> {
        if count > (self.remaining() / 2) as u64 {
            return Err(fatal_host_error("truncated native outcome map"));
        }
        // Fixed structs/enums allocate no map. JSON uses BTreeMap<String, Value>:
        // reserve a whole root node plus 256 bytes per entry, including spare
        // node slots and edges at the tree's minimum non-root occupancy.
        if self.dynamic_maps && count != 0 {
            self.add(1024u64.saturating_add(count.saturating_mul(256)))?;
        }
        for _ in 0..count {
            self.value(depth + 1)?;
            self.value(depth + 1)?;
        }
        Ok(())
    }
    fn payload(&mut self, length: u64) -> wasmtime::Result<()> {
        let length = usize::try_from(length).map_err(fatal_host_error)?;
        self.skip(length)?;
        // One owned payload and the decoder's geometrically grown scratch.
        self.add((length as u64).saturating_mul(3))
    }
    fn integer(&mut self, length: usize) -> wasmtime::Result<u64> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| fatal_host_error("native outcome position overflow"))?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| fatal_host_error("truncated native outcome"))?;
        let mut value = 0u64;
        for byte in bytes {
            value = (value << 8) | u64::from(*byte);
        }
        self.position = end;
        Ok(value)
    }
    fn skip(&mut self, length: usize) -> wasmtime::Result<()> {
        if length > self.remaining() {
            return Err(fatal_host_error("truncated native outcome payload"));
        }
        self.position += length;
        Ok(())
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
    fn add(&mut self, bytes: u64) -> wasmtime::Result<()> {
        self.heap = self
            .heap
            .checked_add(bytes)
            .ok_or_else(|| fatal_host_error("native outcome allocation overflow"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_forged_lengths_before_deserialization() {
        for bytes in [
            vec![0xdb, 0xff, 0xff, 0xff, 0xff],
            vec![0xc6, 0xff, 0xff, 0xff, 0xff],
            vec![0xdd, 0xff, 0xff, 0xff, 0xff],
            vec![0xdf, 0xff, 0xff, 0xff, 0xff],
            vec![0xc0, 0xc0],
            vec![0xc1],
        ] {
            assert!(decoded_heap_bound(&bytes, 32, true).is_err());
        }
        assert!(decoded_heap_bound(&[0x91; 129], 32, true).is_err());
    }
    #[test]
    fn sequences_require_an_explicit_element_bound() {
        assert!(decoded_heap_bound(&[0x91, 0x01], 0, false).is_err());
        assert!(decoded_heap_bound(&[0x91, 0x01], 32, false).unwrap() >= 8 * 32);
        let bytes = [0x81, 0xa1, b'x', 0x01];
        assert!(
            decoded_heap_bound(&bytes, 0, true).unwrap()
                > decoded_heap_bound(&bytes, 0, false).unwrap()
        );
    }
}
