use crate::archive::ArchiveSession;
use crate::build::build_archive;
use crate::error::{Result, WorkflowError};
use crate::model::{
    ArchiveHandle, BuildPlan, BuiltArchive, EditableEntry, ExtractedFile, NamedBytes, SessionId,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct ArchiveService {
    next_session: u64,
    sessions: HashMap<SessionId, ArchiveSession>,
}

impl ArchiveService {
    pub fn open(
        &mut self,
        main_name: String,
        main_bytes: Vec<u8>,
        auxiliary_files: Vec<NamedBytes>,
    ) -> Result<ArchiveHandle> {
        let session = ArchiveSession::open(main_name, main_bytes, auxiliary_files)?;
        Ok(self.insert(session))
    }

    pub fn build(&mut self, plan: BuildPlan) -> Result<BuiltArchive> {
        let volumes = build_archive(plan)?;
        let Some(main) = volumes.first() else {
            return Err(WorkflowError::invalid(
                "archive builder returned no primary volume",
            ));
        };
        let session = ArchiveSession::open(
            main.name.clone(),
            main.bytes.clone(),
            volumes.iter().skip(1).cloned().collect(),
        )?;
        let archive = self.insert(session);
        Ok(BuiltArchive { volumes, archive })
    }

    pub fn supply_volumes(
        &mut self,
        session_id: SessionId,
        files: Vec<NamedBytes>,
    ) -> Result<ArchiveHandle> {
        let session = self.session_mut(session_id)?;
        session.supply_volumes(files)?;
        Ok(ArchiveHandle {
            session_id,
            summary: session.summary().clone(),
        })
    }

    pub fn read_entries(
        &mut self,
        session_id: SessionId,
        entry_ids: &[usize],
    ) -> Result<Vec<ExtractedFile>> {
        self.session_mut(session_id)?.read_entries(entry_ids)
    }

    pub fn editable_entries(
        &mut self,
        session_id: SessionId,
        entry_ids: &[usize],
    ) -> Result<Vec<EditableEntry>> {
        self.session_mut(session_id)?.editable_entries(entry_ids)
    }

    pub fn close(&mut self, session_id: SessionId) -> bool {
        self.sessions.remove(&session_id).is_some()
    }

    pub fn contains(&self, session_id: SessionId) -> bool {
        self.sessions.contains_key(&session_id)
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    fn insert(&mut self, session: ArchiveSession) -> ArchiveHandle {
        self.next_session = self.next_session.wrapping_add(1).max(1);
        let session_id = SessionId(self.next_session);
        let summary = session.summary().clone();
        self.sessions.insert(session_id, session);
        ArchiveHandle {
            session_id,
            summary,
        }
    }

    fn session_mut(&mut self, session_id: SessionId) -> Result<&mut ArchiveSession> {
        self.sessions
            .get_mut(&session_id)
            .ok_or(WorkflowError::SessionNotFound(session_id.0))
    }
}
