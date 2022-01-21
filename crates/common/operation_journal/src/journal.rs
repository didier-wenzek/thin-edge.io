use std::collections::HashMap;
use std::path::Path;
use rustbreak::PathDatabase;
use rustbreak::deser::Ron;

use crate::*;

type JournalData = HashMap<RequestId, Event>;

pub struct Journal {
    db: PathDatabase<JournalData, Ron>,
}

impl Journal {

    pub fn open(path: &str) -> Result<Journal, JournalError> {
        let path = Path::new(path).to_path_buf();
        let empty = HashMap::new();
        let db = PathDatabase::<JournalData, Ron>::load_from_path_or(path, empty)?;

        Ok(Journal {
            db
        })
    }

    pub fn log(&self, event: Event) -> Result<(), JournalError> {
        {
            let mut data = self.db.borrow_data_mut()?;
            data.insert(event.id.clone(), event);
        }
        Ok(self.db.save()?)
    }

    pub fn get(&self, id: &str) -> Result<Option<Event>, JournalError> {
        let data = self.db.borrow_data()?;
        Ok(data.get(id).map(|e| e.clone()))
    }


}