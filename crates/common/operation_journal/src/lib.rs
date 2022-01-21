mod events;
mod journal;
mod errors;

pub use events::*;
pub use journal::*;
pub use errors::*;


#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn it_works() {
        let journal = Journal::open("operations.log").expect("an empty journal or the former one");
        journal.log(Event::schedule(
            12345,
            "op#1",
            "c8y_remote_access",
            "/usr/bin/c8y_remote_access")
        ).expect("an updated journal");

        let event = journal.get("op#1")
            .expect("no database error")
            .expect("the freshly added event");
        assert_eq!(event.time, 12345);
        assert_eq!(&event.id, "op#1");
        if let OperationEvent::Schedule(event) = event.event {
            assert_eq!(&event.operation, "c8y_remote_access");
            assert_eq!(&event.command, "/usr/bin/c8y_remote_access");
        } else {
            panic!("a schedule event was expected");
        }

    }
}
