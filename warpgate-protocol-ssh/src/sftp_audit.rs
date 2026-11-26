use std::collections::HashMap;
use std::io;

use bytes::{Buf, BytesMut};
use tracing::{info, warn};
use uuid::Uuid;
use warpgate_common::UserSessionId;

// OpenSSH's limit is 256 KiB. Allow headroom for other v3 implementations.
const MAX_PACKET_SIZE: usize = 512 * 1024;
const MAX_IN_FLIGHT: usize = 1024;
const MAX_HANDLES: usize = 1024;
const MAX_PATH_SIZE: usize = 4096;
const MAX_HANDLE_SIZE: usize = 256;

#[derive(Default)]
struct Frames(BytesMut);

impl Frames {
    fn feed(
        &mut self,
        mut data: &[u8],
        mut on_frame: impl FnMut(&[u8]) -> io::Result<()>,
    ) -> io::Result<()> {
        while !data.is_empty() {
            if self.0.len() < 4 {
                let take = data.len().min(4 - self.0.len());
                self.0.extend_from_slice(&data[..take]);
                data = &data[take..];
                if self.0.len() < 4 {
                    break;
                }
            }
            let size = self.0.as_ref().get_u32() as usize;
            if !(1..=MAX_PACKET_SIZE).contains(&size) {
                return Err(invalid("unsupported SFTP packet size"));
            }
            let take = data.len().min(size + 4 - self.0.len());
            self.0.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.0.len() == size + 4 {
                on_frame(&self.0[4..])?;
                self.0.clear();
            }
        }
        Ok(())
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn string<'a>(packet: &mut &'a [u8], max: usize) -> io::Result<&'a [u8]> {
    let size = packet
        .try_get_u32()
        .map_err(|_| invalid("truncated SFTP string length"))? as usize;
    if size > max || size > packet.len() {
        return Err(invalid("invalid SFTP string size"));
    }
    let (value, rest) = packet.split_at(size);
    *packet = rest;
    Ok(value)
}

fn path(packet: &mut &[u8]) -> io::Result<String> {
    let value = string(packet, MAX_PATH_SIZE)?;
    Ok(match std::str::from_utf8(value) {
        Ok(value) => value.to_owned(),
        Err(_) => value.escape_ascii().to_string(),
    })
}

#[derive(Default)]
struct Protocol {
    pending: HashMap<u32, Request>,
    handles: HashMap<Vec<u8>, String>,
    v3: bool,
}

struct Request {
    operation: &'static str,
    path: Option<String>,
    second_path: Option<String>,
    handle: Option<Vec<u8>>,
    offset: Option<u64>,
    bytes_requested: Option<u32>,
    flags: Option<u32>,
}

impl Protocol {
    fn request(&mut self, mut packet: &[u8]) -> io::Result<()> {
        let kind = packet.get_u8();
        if kind == 1 {
            return Ok(()); // SSH_FXP_INIT. The server selects the version.
        }
        let id = packet
            .try_get_u32()
            .map_err(|_| invalid("missing SFTP request id"))?;
        let operation = match kind {
            3 => "open",
            4 => "close",
            5 => "read",
            6 => "write",
            7 => "lstat",
            8 => "fstat",
            9 => "setstat",
            10 => "fsetstat",
            11 => "opendir",
            12 => "readdir",
            13 => "remove",
            14 => "mkdir",
            15 => "rmdir",
            16 => "realpath",
            17 => "stat",
            18 => "rename",
            19 => "readlink",
            20 => "symlink",
            200 => "extension",
            _ => "unknown",
        };
        let mut request = Request {
            operation,
            path: None,
            second_path: None,
            handle: None,
            offset: None,
            bytes_requested: None,
            flags: None,
        };
        match kind {
            4..=6 | 8 | 10 | 12 => {
                let handle = string(&mut packet, MAX_HANDLE_SIZE)?;
                request.path = self.handles.get(handle).cloned();
                if kind == 4 {
                    request.handle = Some(handle.to_vec());
                }
                if matches!(kind, 5 | 6) {
                    request.offset = Some(
                        packet
                            .try_get_u64()
                            .map_err(|_| invalid("missing SFTP offset"))?,
                    );
                    request.bytes_requested = Some(if kind == 5 {
                        packet
                            .try_get_u32()
                            .map_err(|_| invalid("missing SFTP read length"))?
                    } else {
                        string(&mut packet, MAX_PACKET_SIZE)?.len() as u32
                    });
                }
            }
            3 | 7 | 9 | 11 | 13..=20 => {
                request.path = Some(path(&mut packet)?);
                if matches!(kind, 18 | 20) {
                    request.second_path = Some(path(&mut packet)?);
                }
                if kind == 3 {
                    request.flags = Some(
                        packet
                            .try_get_u32()
                            .map_err(|_| invalid("missing SFTP open flags"))?,
                    );
                }
            }
            200 => match string(&mut packet, MAX_PATH_SIZE)? {
                b"posix-rename@openssh.com" | b"hardlink@openssh.com" => {
                    request.operation = "rename_or_link";
                    request.path = Some(path(&mut packet)?);
                    request.second_path = Some(path(&mut packet)?);
                }
                _ => {}
            },
            _ => {}
        }
        if self.pending.len() >= MAX_IN_FLIGHT || self.pending.insert(id, request).is_some() {
            return Err(invalid(
                "duplicate SFTP request id or too many pending requests",
            ));
        }
        Ok(())
    }

    fn response(
        &mut self,
        mut packet: &[u8],
        mut report: impl FnMut(Request, u32, Option<u32>),
    ) -> io::Result<()> {
        let kind = packet.get_u8();
        if kind == 2 {
            self.v3 = packet.try_get_u32().ok() == Some(3);
            return if self.v3 {
                Ok(())
            } else {
                Err(invalid("unsupported SFTP version"))
            };
        }
        if !self.v3 {
            return Err(invalid("missing SFTP version response"));
        }
        let id = packet
            .try_get_u32()
            .map_err(|_| invalid("missing SFTP response id"))?;
        let request = self
            .pending
            .remove(&id)
            .ok_or_else(|| invalid("unmatched SFTP response id"))?;
        let mut transferred = None;
        let status = match kind {
            101 => {
                let status = packet
                    .try_get_u32()
                    .map_err(|_| invalid("missing SFTP status"))?;
                if status == 0 {
                    if let Some(handle) = &request.handle {
                        self.handles.remove(handle);
                    }
                    if request.operation == "write" {
                        transferred = request.bytes_requested;
                    }
                }
                status
            }
            102 => {
                let handle = string(&mut packet, MAX_HANDLE_SIZE)?;
                if let Some(path) = &request.path {
                    if self.handles.len() >= MAX_HANDLES {
                        return Err(invalid("too many tracked SFTP handles"));
                    }
                    self.handles.insert(handle.to_vec(), path.clone());
                }
                0
            }
            103 => {
                transferred = Some(string(&mut packet, MAX_PACKET_SIZE)?.len() as u32);
                0
            }
            104 | 105 | 201 => 0, // NAME, ATTRS and EXTENDED_REPLY.
            _ => return Err(invalid("unsupported SFTP response")),
        };
        report(request, status, transferred);
        Ok(())
    }
}

#[derive(Default)]
pub struct SftpAudit {
    input: Frames,
    output: Frames,
    protocol: Protocol,
}

impl SftpAudit {
    pub fn observe(
        &mut self,
        session: UserSessionId,
        channel_id: Uuid,
        data: &[u8],
        from_client: bool,
    ) -> io::Result<()> {
        let protocol = &mut self.protocol;
        if from_client {
            return self.input.feed(data, |packet| protocol.request(packet));
        }
        self.output.feed(data, |packet| {
            protocol.response(packet, |request, status, transferred| {
                info!(
                    target: "audit",
                    _type = "SshSftpOperation1",
                    %session,
                    %channel_id,
                    operation = request.operation,
                    path = request.path.as_deref(),
                    second_path = request.second_path.as_deref(),
                    offset = request.offset,
                    bytes_requested = request.bytes_requested,
                    bytes_transferred = transferred,
                    open_flags = request.flags,
                    status,
                    "Observed SFTP operation result"
                );
            })
        })
    }

    pub fn report_unanswered(&self, session: UserSessionId, channel_id: Uuid) {
        let pending = self.protocol.pending.len();
        if pending != 0 {
            warn!(
                target: "audit",
                _type = "SshSftpUnanswered1",
                %session,
                %channel_id,
                pending,
                "SFTP channel closed with unanswered requests"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::BufMut;

    use super::*;

    fn frame(kind: u8, id: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.put_u32(5 + payload.len() as u32);
        bytes.put_u8(kind);
        bytes.put_u32(id);
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn tracks_handles_and_transfer_results_across_fragmented_packets() {
        let mut input = Frames::default();
        let mut output = Frames::default();
        let mut protocol = Protocol::default();
        let mut events = Vec::new();
        let mut report = |request, status, transferred| events.push((request, status, transferred));

        let open = frame(3, 1, b"\x00\x00\x00\x02/a\x00\x00\x00\x02\0\0\0\0");
        for chunk in open.chunks(3) {
            input
                .feed(chunk, |packet| protocol.request(packet))
                .unwrap();
        }
        let mut initial = frame(2, 3, b"");
        initial.extend(frame(102, 1, b"\x00\x00\x00\x01h"));
        output
            .feed(&initial, |packet| protocol.response(packet, &mut report))
            .unwrap();

        let mut write = Vec::from(b"\x00\x00\x00\x01h".as_slice());
        write.put_u64(42);
        write.put_u32(3);
        write.extend(b"abc");
        input
            .feed(&frame(6, 7, &write), |packet| protocol.request(packet))
            .unwrap();
        output
            .feed(&frame(101, 7, &[0; 4]), |packet| {
                protocol.response(packet, &mut report)
            })
            .unwrap();

        assert_eq!(events.len(), 2);
        let (written, status, bytes) = &events[1];
        assert_eq!(written.operation, "write");
        assert_eq!(written.path.as_deref(), Some("/a"));
        assert_eq!(written.offset, Some(42));
        assert_eq!((*status, *bytes), (0, Some(3)));
    }
}
