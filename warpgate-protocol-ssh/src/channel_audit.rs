use tracing::{info, warn};
use uuid::Uuid;
use warpgate_common::UserSessionId;
use warpgate_core::recordings::{self, TerminalRecorder, TerminalRecordingStreamId};

use crate::command_detector::CommandDetector;
use crate::common::PtyRequest;
use crate::sftp_audit::SftpAudit;

/// Encapsulates channel recording and command detection
pub struct ChannelAudit {
    session_id: UserSessionId,
    channel_id: Uuid,
    recording: Recording,
    detector: Option<CommandDetector>,
    sftp: Option<Box<SftpAudit>>,
    sftp_requested: bool,
}

enum Recording {
    Off,
    /// The recording starts later. Input received until then still reaches
    /// the target, so it is written to the recording when it starts
    Pending(Vec<u8>),
    On(TerminalRecorder),
}

impl ChannelAudit {
    pub const fn new(session_id: UserSessionId, channel_id: Uuid) -> Self {
        Self {
            session_id,
            channel_id,
            recording: Recording::Off,
            detector: None,
            sftp: None,
            sftp_requested: false,
        }
    }

    pub fn await_recorder(&mut self) {
        if matches!(self.recording, Recording::Off) {
            self.recording = Recording::Pending(vec![]);
        }
    }

    /// `None` when recording is disabled
    pub async fn set_recorder(
        &mut self,
        recorder: Option<TerminalRecorder>,
    ) -> recordings::Result<()> {
        let Some(recorder) = recorder else {
            self.recording = Recording::Off;
            return Ok(());
        };
        if let Recording::Pending(input) = &self.recording
            && !input.is_empty()
        {
            recorder
                .write(TerminalRecordingStreamId::Input, input)
                .await?;
        }
        self.recording = Recording::On(recorder);
        Ok(())
    }

    pub fn start_command_detection(&mut self, cols: u16, rows: u16) {
        self.detector = Some(CommandDetector::new(cols, rows));
    }

    pub fn start_sftp_audit(&mut self) {
        if !self.sftp_requested {
            self.sftp_requested = true;
            self.sftp = Some(Box::default());
        }
    }

    fn observe_sftp(&mut self, data: &[u8], from_client: bool) {
        if let Some(sftp) = &mut self.sftp
            && let Err(error) = sftp.observe(self.session_id, self.channel_id, data, from_client)
        {
            warn!(
                target: "audit",
                _type = "SshSftpAuditGap1",
                session = %self.session_id,
                channel_id = %self.channel_id,
                reason = %error,
                "Could not continue parsing SFTP metadata"
            );
            self.sftp = None;
        }
    }

    pub async fn on_input(&mut self, data: &[u8]) -> recordings::Result<()> {
        self.observe_sftp(data, true);
        self.record(TerminalRecordingStreamId::Input, data).await?;
        if let Some(detector) = self.detector.as_mut() {
            detector.on_input(data);
        }
        Ok(())
    }

    pub async fn on_output(&mut self, data: &[u8]) -> recordings::Result<()> {
        self.observe_sftp(data, false);
        self.record(TerminalRecordingStreamId::Output, data).await?;
        if let Some(detector) = self.detector.as_mut()
            && let Some(command) = detector.on_output(data)
        {
            info!(
                target: "audit",
                _type = "SshShellCommandObserved1",
                session = %self.session_id,
                channel_id = %self.channel_id,
                %command,
                "Observed SSH shell command"
            );
        }
        Ok(())
    }

    pub async fn on_error_output(&mut self, data: &[u8]) -> recordings::Result<()> {
        self.record(TerminalRecordingStreamId::Error, data).await
    }

    pub async fn on_resize(&mut self, pty: &PtyRequest) -> recordings::Result<()> {
        if let Recording::On(recorder) = &self.recording {
            recorder
                .write_pty_resize(pty.col_width, pty.row_height)
                .await?;
        }
        if let Some(detector) = self.detector.as_mut() {
            let (cols, rows) = pty.screen_size();
            detector.on_resize(cols, rows);
        }
        Ok(())
    }

    async fn record(
        &mut self,
        stream: TerminalRecordingStreamId,
        data: &[u8],
    ) -> recordings::Result<()> {
        match (&mut self.recording, stream) {
            (Recording::On(recorder), stream) => recorder.write(stream, data).await,
            (Recording::Pending(input), TerminalRecordingStreamId::Input) => {
                input.extend_from_slice(data);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl Drop for ChannelAudit {
    fn drop(&mut self) {
        if let Some(sftp) = &self.sftp {
            sftp.report_unanswered(self.session_id, self.channel_id);
        }
    }
}
