use mqttrs::Pid;

#[derive(Default)]
pub struct Session {
    pid: Pid,
}

impl Session {
    pub fn next_pid(&mut self) -> Pid {
        let pid = self.pid;
        self.pid = self.pid + 1;
        pid
    }
}
