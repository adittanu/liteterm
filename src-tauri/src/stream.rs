//! Terminal sessions that are a plain byte stream instead of SSH: Telnet
//! (RFC 854) and serial consoles (switches, routers, boards on a USB-serial
//! cable).
//!
//! They share the local terminal's output path (`local::flush_loop` and the
//! `ssh:stdout` event) and its status events, so the frontend treats them as
//! one more kind of tab. There is no SFTP, port forwarding, agent or host key
//! here, and no automatic reconnect: Reconnect in the tab opens a new session.
//!
//! Blocking I/O on plain threads, like the local terminal: one reader per
//! session, and writes from the command that delivers keystrokes.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::ssh::pool::{emit_status, AppState};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How often a serial reader looks up from an idle line to see whether the
/// session was closed
const SERIAL_READ_TIMEOUT: Duration = Duration::from_millis(200);
pub const DEFAULT_BAUD: u32 = 115_200;

pub enum Target {
    Telnet { address: String, port: u16 },
    Serial { device: String, baud: u32 },
}

pub struct StreamSession {
    pub id: u32,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    /// Telnet only: the option state, shared by the reader (which answers the
    /// server) and resize (which reports the window size)
    telnet: Option<Mutex<telnet::Telnet>>,
    /// Telnet only: shut down on close, which wakes the blocked reader
    tcp: Mutex<Option<TcpStream>>,
    closed: CancellationToken,
}

impl StreamSession {
    /// Bytes as they go on the wire; a write error surfaces through the reader
    fn write_raw(&self, bytes: &[u8]) -> Result<(), String> {
        let mut guard = self.writer.lock().unwrap();
        let writer = guard.as_mut().ok_or("Not connected")?;
        writer.write_all(bytes).map_err(|e| format!("Write failed: {e}"))?;
        writer.flush().map_err(|e| format!("Write failed: {e}"))
    }
}

/// Start connecting and return the session id at once. The outcome arrives
/// as a status event ("connected", or "error" with the reason), the same way
/// an SSH session reports it, so the tab can show it.
pub fn open(state: &Arc<AppState>, target: Target) -> u32 {
    let id = state.next_session_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let session = Arc::new(StreamSession {
        id,
        writer: Mutex::new(None),
        telnet: matches!(target, Target::Telnet { .. }).then(|| Mutex::new(telnet::Telnet::default())),
        tcp: Mutex::new(None),
        closed: CancellationToken::new(),
    });
    state.stream_sessions.lock().unwrap().insert(id, session.clone());
    let app = state.app.clone();
    std::thread::spawn(move || run(app, session, target));
    id
}

fn run(app: tauri::AppHandle, session: Arc<StreamSession>, target: Target) {
    let reader = match connect(&session, &target) {
        Ok(reader) => reader,
        Err(e) => {
            if !session.closed.is_cancelled() {
                emit_status(&app, session.id, "error", Some(e));
            }
            return;
        }
    };
    // Closed while it was still connecting: nobody is waiting for it
    if session.closed.is_cancelled() {
        return;
    }
    emit_status(&app, session.id, "connected", None);

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let id = session.id;
    let flush_app = app.clone();
    let flusher = std::thread::spawn(move || {
        crate::local::flush_loop(rx, |bytes| crate::local::emit_stdout(&flush_app, id, bytes));
    });
    pump(&session, reader, tx);
    let _ = flusher.join();
    session.closed.cancel();
    emit_status(&app, id, "closed", None);
}

fn connect(session: &StreamSession, target: &Target) -> Result<Box<dyn Read + Send>, String> {
    match target {
        Target::Telnet { address, port } => {
            let addrs: Vec<_> = (address.as_str(), *port)
                .to_socket_addrs()
                .map_err(|e| format!("Could not find {address}: {e}"))?
                .collect();
            let mut last_error = None;
            let mut stream = None;
            for addr in addrs {
                match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                    Ok(s) => {
                        stream = Some(s);
                        break;
                    }
                    Err(e) => last_error = Some(e),
                }
            }
            let stream = stream.ok_or_else(|| match last_error {
                Some(e) => format!("Could not connect to {address}:{port}: {e}"),
                None => format!("{address} has no address to connect to"),
            })?;
            let _ = stream.set_nodelay(true);
            let clone = |s: &TcpStream| s.try_clone().map_err(|e| format!("Could not set up the connection: {e}"));
            *session.tcp.lock().unwrap() = Some(clone(&stream)?);
            *session.writer.lock().unwrap() = Some(Box::new(clone(&stream)?));
            // Offer the window size and terminal type up front; most servers
            // ask for them, and some wait to be offered
            let hello = session.telnet.as_ref().map(|t| t.lock().unwrap().greeting()).unwrap_or_default();
            session.write_raw(&hello)?;
            Ok(Box::new(stream))
        }
        Target::Serial { device, baud } => {
            // 8 data bits, no parity, 1 stop bit, no flow control: what
            // consoles almost always use
            let port = serialport::new(device, *baud)
                .timeout(SERIAL_READ_TIMEOUT)
                .open()
                .map_err(|e| serial_error(device, e))?;
            let writer = port.try_clone().map_err(|e| serial_error(device, e))?;
            *session.writer.lock().unwrap() = Some(Box::new(writer));
            Ok(Box::new(port))
        }
    }
}

/// What to do about a port that won't open, in words a person can act on
fn serial_error(device: &str, e: serialport::Error) -> String {
    use serialport::ErrorKind;
    match e.kind() {
        ErrorKind::NoDevice => format!("{device} is not there. Plug the adapter in, or pick another port"),
        ErrorKind::Io(std::io::ErrorKind::NotFound) => {
            format!("{device} is not there. Plug the adapter in, or pick another port")
        }
        ErrorKind::Io(std::io::ErrorKind::PermissionDenied) if cfg!(target_os = "linux") => format!(
            "No permission to open {device}. Add yourself to the dialout group \
             (sudo usermod -aG dialout $USER), then log out and back in"
        ),
        ErrorKind::Io(std::io::ErrorKind::PermissionDenied) => {
            format!("{device} is in use by another program, or access to it was denied")
        }
        ErrorKind::InvalidInput => format!("{device} does not take these settings: {e}"),
        _ => format!("Could not open {device}: {e}"),
    }
}

fn pump(session: &StreamSession, mut reader: Box<dyn Read + Send>, tx: mpsc::Sender<Vec<u8>>) {
    use std::io::ErrorKind;

    let mut buf = [0u8; 16 * 1024];
    loop {
        if session.closed.is_cancelled() {
            break;
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            // An idle serial line times out; a signal interrupts
            Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted) => continue,
            Err(_) => break,
        };
        let data = match &session.telnet {
            None => buf[..n].to_vec(),
            Some(t) => {
                let (data, reply) = t.lock().unwrap().feed(&buf[..n]);
                if !reply.is_empty() && session.write_raw(&reply).is_err() {
                    break;
                }
                data
            }
        };
        if !data.is_empty() && tx.send(data).is_err() {
            break;
        }
    }
}

/// Keystrokes from the terminal
pub fn write(session: &StreamSession, data: &str) -> Result<(), String> {
    match &session.telnet {
        Some(_) => session.write_raw(&telnet::encode(data.as_bytes())),
        None => session.write_raw(data.as_bytes()),
    }
}

/// A serial line has no window size; Telnet reports it once the server
/// agreed to hear it (NAWS)
pub fn resize(session: &StreamSession, cols: u16, rows: u16) -> Result<(), String> {
    let Some(t) = &session.telnet else { return Ok(()) };
    let report = t.lock().unwrap().resize(cols, rows);
    match report {
        Some(bytes) => session.write_raw(&bytes),
        None => Ok(()),
    }
}

pub fn close(state: &AppState, session: &StreamSession) {
    session.closed.cancel();
    if let Some(tcp) = session.tcp.lock().unwrap().take() {
        let _ = tcp.shutdown(std::net::Shutdown::Both);
    }
    *session.writer.lock().unwrap() = None;
    state.stream_sessions.lock().unwrap().remove(&session.id);
}

#[derive(Serialize)]
pub struct SerialPortEntry {
    /// What to open: COM3, /dev/ttyUSB0, /dev/cu.usbserial-1410
    pub name: String,
    /// The adapter, when the system says (e.g. "CP2102 USB to UART")
    pub description: Option<String>,
}

pub fn serial_ports() -> Vec<SerialPortEntry> {
    use serialport::SerialPortType;

    let mut ports: Vec<SerialPortEntry> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        // macOS lists each device twice; the tty.* one waits for carrier
        // detect, so cu.* is the one a console cable answers on
        .filter(|p| !p.port_name.starts_with("/dev/tty.") || cfg!(not(target_os = "macos")))
        .map(|p| SerialPortEntry {
            description: match p.port_type {
                SerialPortType::UsbPort(usb) => usb.product.or(usb.manufacturer),
                SerialPortType::BluetoothPort => Some("Bluetooth".into()),
                SerialPortType::PciPort | SerialPortType::Unknown => None,
            },
            name: p.port_name,
        })
        .collect();
    ports.sort_by(|a, b| natural_key(&a.name).cmp(&natural_key(&b.name)));
    ports
}

/// COM2 before COM10
fn natural_key(name: &str) -> (String, u64) {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (stem, num) = name.split_at(name.len() - digits);
    (stem.to_string(), num.parse().unwrap_or(0))
}

/// Telnet option negotiation (RFC 854/855), as a pure state machine: bytes
/// from the server go in, terminal data and replies for the server come out.
pub mod telnet {
    pub const IAC: u8 = 255;
    pub const DONT: u8 = 254;
    pub const DO: u8 = 253;
    pub const WONT: u8 = 252;
    pub const WILL: u8 = 251;
    pub const SB: u8 = 250;
    pub const SE: u8 = 240;

    pub const ECHO: u8 = 1;
    pub const SGA: u8 = 3;
    pub const TTYPE: u8 = 24;
    pub const NAWS: u8 = 31;

    const TTYPE_IS: u8 = 0;
    const TTYPE_SEND: u8 = 1;
    const TERMINAL_TYPE: &[u8] = b"XTERM-256COLOR";
    /// Longest subnegotiation kept; anything past it is dropped
    const MAX_SB: usize = 1024;

    #[derive(Clone, Copy)]
    enum State {
        Data,
        Iac,
        Verb(u8),
        SbOption,
        Sb,
        SbIac,
    }

    /// Options this side performs when asked (DO)
    fn ours(opt: u8) -> bool {
        matches!(opt, SGA | TTYPE | NAWS)
    }

    /// Options this side lets the server perform (WILL)
    fn theirs(opt: u8) -> bool {
        matches!(opt, ECHO | SGA)
    }

    pub struct Telnet {
        state: State,
        sb_option: u8,
        sb: Vec<u8>,
        /// Options in effect on this side, and on the server's
        us: [bool; 256],
        him: [bool; 256],
        /// Requests this side sent and is waiting on, so the answer to one
        /// is not answered again (that is how negotiation loops start)
        us_asked: [bool; 256],
        him_asked: [bool; 256],
        cols: u16,
        rows: u16,
        /// The last data byte was CR: a NUL after it only marks a bare CR
        cr: bool,
    }

    impl Default for Telnet {
        fn default() -> Self {
            Telnet {
                state: State::Data,
                sb_option: 0,
                sb: Vec::new(),
                us: [false; 256],
                him: [false; 256],
                us_asked: [false; 256],
                him_asked: [false; 256],
                cols: 80,
                rows: 24,
                cr: false,
            }
        }
    }

    impl Telnet {
        /// Sent once connected: offer the window size and terminal type, and
        /// ask the server to go character at a time
        pub fn greeting(&mut self) -> Vec<u8> {
            let mut out = Vec::new();
            for opt in [NAWS, TTYPE] {
                self.us_asked[opt as usize] = true;
                out.extend([IAC, WILL, opt]);
            }
            self.him_asked[SGA as usize] = true;
            out.extend([IAC, DO, SGA]);
            out
        }

        /// Bytes from the server: (data for the terminal, replies to send)
        pub fn feed(&mut self, input: &[u8]) -> (Vec<u8>, Vec<u8>) {
            let mut data = Vec::with_capacity(input.len());
            let mut reply = Vec::new();
            for &b in input {
                self.state = match self.state {
                    State::Data if b == IAC => State::Iac,
                    State::Data => {
                        self.data(b, &mut data);
                        State::Data
                    }
                    State::Iac => match b {
                        IAC => {
                            self.data(IAC, &mut data);
                            State::Data
                        }
                        WILL | WONT | DO | DONT => State::Verb(b),
                        SB => State::SbOption,
                        // NOP, GA, AYT and the rest carry nothing to show
                        _ => State::Data,
                    },
                    State::Verb(verb) => {
                        self.negotiate(verb, b, &mut reply);
                        State::Data
                    }
                    State::SbOption => {
                        self.sb_option = b;
                        self.sb.clear();
                        State::Sb
                    }
                    State::Sb if b == IAC => State::SbIac,
                    State::Sb => {
                        if self.sb.len() < MAX_SB {
                            self.sb.push(b);
                        }
                        State::Sb
                    }
                    State::SbIac => match b {
                        SE => {
                            self.subnegotiation(&mut reply);
                            State::Data
                        }
                        IAC => {
                            if self.sb.len() < MAX_SB {
                                self.sb.push(IAC);
                            }
                            State::Sb
                        }
                        _ => State::Data,
                    },
                };
            }
            (data, reply)
        }

        fn data(&mut self, b: u8, data: &mut Vec<u8>) {
            if self.cr && b == 0 {
                self.cr = false;
                return;
            }
            self.cr = b == b'\r';
            data.push(b);
        }

        fn negotiate(&mut self, verb: u8, opt: u8, reply: &mut Vec<u8>) {
            let o = opt as usize;
            match verb {
                DO => {
                    if ours(opt) {
                        if !self.us[o] {
                            self.us[o] = true;
                            if !self.us_asked[o] {
                                reply.extend([IAC, WILL, opt]);
                            }
                        }
                        if opt == NAWS {
                            reply.extend(self.naws());
                        }
                    } else {
                        reply.extend([IAC, WONT, opt]);
                    }
                    self.us_asked[o] = false;
                }
                DONT => {
                    if self.us[o] {
                        self.us[o] = false;
                        reply.extend([IAC, WONT, opt]);
                    }
                    self.us_asked[o] = false;
                }
                WILL => {
                    if theirs(opt) {
                        if !self.him[o] {
                            self.him[o] = true;
                            if !self.him_asked[o] {
                                reply.extend([IAC, DO, opt]);
                            }
                        }
                    } else {
                        reply.extend([IAC, DONT, opt]);
                    }
                    self.him_asked[o] = false;
                }
                WONT => {
                    if self.him[o] {
                        self.him[o] = false;
                        reply.extend([IAC, DONT, opt]);
                    }
                    self.him_asked[o] = false;
                }
                _ => {}
            }
        }

        fn subnegotiation(&mut self, reply: &mut Vec<u8>) {
            if self.sb_option == TTYPE && self.sb.first() == Some(&TTYPE_SEND) && self.us[TTYPE as usize] {
                reply.extend([IAC, SB, TTYPE, TTYPE_IS]);
                reply.extend_from_slice(TERMINAL_TYPE);
                reply.extend([IAC, SE]);
            }
        }

        fn naws(&self) -> Vec<u8> {
            let mut out = vec![IAC, SB, NAWS];
            for b in [(self.cols >> 8) as u8, self.cols as u8, (self.rows >> 8) as u8, self.rows as u8] {
                out.push(b);
                if b == IAC {
                    out.push(IAC);
                }
            }
            out.extend([IAC, SE]);
            out
        }

        /// The terminal's new size, and the report to send if the server
        /// asked for sizes
        pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
            self.cols = cols;
            self.rows = rows;
            self.us[NAWS as usize].then(|| self.naws())
        }
    }

    /// Keystrokes as Telnet sends them: a data byte 255 is doubled, and Enter
    /// (a CR on its own) becomes CR LF, the Telnet end of line
    pub fn encode(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() + 4);
        for (i, &b) in input.iter().enumerate() {
            match b {
                IAC => out.extend([IAC, IAC]),
                b'\r' => {
                    out.push(b'\r');
                    if input.get(i + 1) != Some(&b'\n') {
                        out.push(b'\n');
                    }
                }
                _ => out.push(b),
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_server_that_agrees_to_our_offers_is_not_answered_twice() {
            let mut t = Telnet::default();
            assert_eq!(t.greeting(), [IAC, WILL, NAWS, IAC, WILL, TTYPE, IAC, DO, SGA]);
            // Its yes to our WILL NAWS: no second WILL, just the size
            let (data, reply) = t.feed(&[IAC, DO, NAWS, IAC, WILL, SGA]);
            assert!(data.is_empty());
            assert_eq!(reply, [IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE]);
            // Asked again later (it forgot): the size again, still no WILL
            let (_, reply) = t.feed(&[IAC, DO, NAWS]);
            assert_eq!(reply, [IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE]);
        }

        #[test]
        fn requests_are_answered_and_unknown_options_refused() {
            let mut t = Telnet::default();
            let (_, reply) = t.feed(&[IAC, WILL, ECHO, IAC, DO, 39, IAC, WILL, 34]);
            assert_eq!(reply, [IAC, DO, ECHO, IAC, WONT, 39, IAC, DONT, 34]);
            // Already on: saying it again changes nothing
            let (_, reply) = t.feed(&[IAC, WILL, ECHO]);
            assert!(reply.is_empty());
            let (_, reply) = t.feed(&[IAC, WONT, ECHO]);
            assert_eq!(reply, [IAC, DONT, ECHO]);
            let (_, reply) = t.feed(&[IAC, WONT, ECHO]);
            assert!(reply.is_empty(), "off is off; no loop");
        }

        #[test]
        fn the_terminal_type_is_sent_when_asked() {
            let mut t = Telnet::default();
            t.feed(&[IAC, DO, TTYPE]);
            let (_, reply) = t.feed(&[IAC, SB, TTYPE, TTYPE_SEND, IAC, SE]);
            let mut want = vec![IAC, SB, TTYPE, TTYPE_IS];
            want.extend_from_slice(b"XTERM-256COLOR");
            want.extend([IAC, SE]);
            assert_eq!(reply, want);
        }

        #[test]
        fn data_comes_through_split_anywhere() {
            let mut t = Telnet::default();
            let mut data = Vec::new();
            let mut reply = Vec::new();
            for chunk in [&b"log"[..], &[b'i', b'n', IAC], &[WILL], &[ECHO, b':', IAC], &[IAC, b' ', b'\r', 0, b'\n']] {
                let (d, r) = t.feed(chunk);
                data.extend(d);
                reply.extend(r);
            }
            assert_eq!(data, [b"login:".as_slice(), &[IAC, b' ', b'\r', b'\n']].concat());
            assert_eq!(reply, [IAC, DO, ECHO]);
        }

        #[test]
        fn sizes_go_out_once_the_server_asked_for_them() {
            let mut t = Telnet::default();
            assert_eq!(t.resize(120, 40), None, "not before DO NAWS");
            let (_, reply) = t.feed(&[IAC, DO, NAWS]);
            assert_eq!(reply, [IAC, WILL, NAWS, IAC, SB, NAWS, 0, 120, 0, 40, IAC, SE]);
            // 255 columns: the byte is doubled inside the subnegotiation
            assert_eq!(t.resize(255, 50), Some(vec![IAC, SB, NAWS, 0, IAC, IAC, 0, 50, IAC, SE]));
        }

        #[test]
        fn keystrokes_are_escaped_and_enter_is_cr_lf() {
            assert_eq!(encode(b"ls\r"), b"ls\r\n");
            assert_eq!(encode(b"a\r\nb"), b"a\r\nb");
            assert_eq!(encode(&[b'x', IAC]), [b'x', IAC, IAC]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use telnet::{DO, IAC, NAWS, SB, SE, SGA, TTYPE, WILL};

    fn session(telnet: bool) -> Arc<StreamSession> {
        Arc::new(StreamSession {
            id: 1,
            writer: Mutex::new(None),
            telnet: telnet.then(|| Mutex::new(telnet::Telnet::default())),
            tcp: Mutex::new(None),
            closed: CancellationToken::new(),
        })
    }

    /// A real socket on localhost, standing in for a router's telnetd
    #[test]
    fn a_telnet_session_negotiates_shows_data_and_sends_keys() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 256];
            // The client's offers first
            while got.len() < 9 {
                let n = sock.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            assert_eq!(&got[..9], [IAC, WILL, NAWS, IAC, WILL, TTYPE, IAC, DO, SGA]);
            got.clear();
            sock.write_all(&[IAC, DO, NAWS, IAC, WILL, SGA]).unwrap();
            sock.write_all(b"Router login: ").unwrap();
            // The window size, then "admin" and Enter
            let want: Vec<u8> = [&[IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE][..], b"admin\r\n"].concat();
            while got.len() < want.len() {
                let n = sock.read(&mut buf).unwrap();
                assert!(n > 0, "client hung up early: {got:?}");
                got.extend_from_slice(&buf[..n]);
            }
            assert_eq!(got, want);
        });

        let s = session(true);
        let reader = connect(&s, &Target::Telnet { address: "127.0.0.1".into(), port }).unwrap();
        let (tx, rx) = mpsc::channel();
        let s2 = s.clone();
        let pumping = std::thread::spawn(move || pump(&s2, reader, tx));

        let mut shown = Vec::new();
        while !shown.ends_with(b"login: ") {
            shown.extend(rx.recv_timeout(Duration::from_secs(5)).expect("no output"));
        }
        assert_eq!(shown, b"Router login: ", "negotiation never reaches the screen");
        write(&s, "admin\r").unwrap();
        server.join().unwrap();

        let state_free_close = |s: &StreamSession| {
            s.closed.cancel();
            if let Some(tcp) = s.tcp.lock().unwrap().take() {
                let _ = tcp.shutdown(std::net::Shutdown::Both);
            }
        };
        state_free_close(&s);
        pumping.join().unwrap();
    }

    #[test]
    fn a_closed_port_says_so() {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);
        let err = connect(&session(true), &Target::Telnet { address: "127.0.0.1".into(), port }).err().unwrap();
        assert!(err.starts_with(&format!("Could not connect to 127.0.0.1:{port}")), "{err}");
    }

    #[test]
    fn a_missing_serial_port_says_so() {
        let device = if cfg!(windows) { "COM250" } else { "/dev/ttyLITETERM-none" };
        let err = connect(&session(false), &Target::Serial { device: device.into(), baud: 9600 }).err().unwrap();
        assert!(err.contains(device), "{err}");
    }

    #[test]
    fn com_ports_sort_by_number() {
        let mut names = vec!["COM10", "COM2", "COM1", "/dev/ttyUSB0"];
        names.sort_by(|a, b| natural_key(a).cmp(&natural_key(b)));
        assert_eq!(names, ["/dev/ttyUSB0", "COM1", "COM2", "COM10"]);
    }
}
