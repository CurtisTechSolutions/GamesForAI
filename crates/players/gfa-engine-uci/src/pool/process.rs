use super::{Deadline, UciError};
use crate::MAX_LINE_BYTES;
use std::{
    io::{Read, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
};

struct WriteRequest {
    text: String,
    done: SyncSender<Result<(), ()>>,
}
pub(super) struct Process {
    child: Child,
    input: Option<SyncSender<WriteRequest>>,
    output: Receiver<String>,
    invalid_output: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
}
impl Process {
    pub(super) fn spawn(mut command: Command) -> Result<Self, UciError> {
        let mut child = command
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().map_err(|_| UciError::Unavailable)?;
        let pipes = child.stdin.take().zip(child.stdout.take());
        let Some((mut stdin, mut stdout)) = pipes else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(UciError::Unavailable);
        };
        // A stalled caller or noisy engine cannot allocate an unbounded queue.
        let (input, commands) = mpsc::sync_channel::<WriteRequest>(1);
        let (lines, output) = mpsc::sync_channel(128);
        let invalid_output = Arc::new(AtomicBool::new(false));
        let mut process = Self {
            child, input: Some(input), output, invalid_output: invalid_output.clone(),
            reader: None, writer: None,
        };
        process.reader = Some(thread::Builder::new().name("gfa-uci-read".into()).spawn(move || {
            let mut chunk = [0_u8; 1024];
            let mut line = Vec::with_capacity(256);
            loop {
                let count = match stdout.read(&mut chunk) {
                    Ok(0) | Err(_) => return,
                    Ok(count) => count,
                };
                for &byte in &chunk[..count] {
                    if byte == b'\n' {
                        let Ok(text) = String::from_utf8(std::mem::take(&mut line)) else {
                            invalid_output.store(true, Ordering::Release);
                            return;
                        };
                        if lines.try_send(text).is_err() {
                            invalid_output.store(true, Ordering::Release);
                            return;
                        }
                    } else if line.len() == MAX_LINE_BYTES {
                        invalid_output.store(true, Ordering::Release);
                        return;
                    } else {
                        line.push(byte);
                    }
                }
            }
        }).map_err(|_| UciError::Unavailable)?);
        process.writer = Some(thread::Builder::new().name("gfa-uci-write".into()).spawn(move || {
            while let Ok(request) = commands.recv() {
                let result = stdin.write_all(request.text.as_bytes())
                    .and_then(|()| stdin.write_all(b"\n")).and_then(|()| stdin.flush())
                    .map_err(|_| ());
                let _ = request.done.send(result);
                if result.is_err() { return; }
            }
        }).map_err(|_| UciError::Unavailable)?);
        Ok(process)
    }

    pub(super) fn send(&self, text: &str, deadline: &Deadline<'_>) -> Result<(), UciError> {
        deadline.check()?;
        if text.len() > MAX_LINE_BYTES || text.contains(['\r', '\n']) {
            return Err(UciError::InvalidInput);
        }
        if self.invalid_output.load(Ordering::Acquire) { return Err(UciError::Protocol); }
        let (done, result) = mpsc::sync_channel(1);
        self.input.as_ref().ok_or(UciError::Disconnected)?
            .try_send(WriteRequest { text: text.into(), done })
            .map_err(|_| UciError::Disconnected)?;
        loop {
            match result.recv_timeout(deadline.tick()?) {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(())) | Err(RecvTimeoutError::Disconnected) => return Err(UciError::Disconnected),
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    pub(super) fn receive(&self, deadline: &Deadline<'_>) -> Result<String, UciError> {
        loop {
            if self.invalid_output.load(Ordering::Acquire) { return Err(UciError::Protocol); }
            match self.output.recv_timeout(deadline.tick()?) {
                Ok(line) => return Ok(line),
                Err(RecvTimeoutError::Disconnected) => return Err(UciError::Disconnected),
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        // In production, bwrap's parent-death and PID namespace policy kills engine descendants too.
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.input.take();
        if let Some(reader) = self.reader.take() { let _ = reader.join(); }
        if let Some(writer) = self.writer.take() { let _ = writer.join(); }
    }
}
