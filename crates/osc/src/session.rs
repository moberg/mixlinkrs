//! IPv4 UDP OSC session: bind `0.0.0.0:listen`, send `host:send_port`.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};

use crate::addresses::{self, Bus};
use crate::codec::{decode, encode, OscMessage, OscValue};

const RCVBUF: usize = 1 << 20;
const DUMP_INTERVAL: Duration = Duration::from_secs(2);
const RECV_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, thiserror::Error)]
pub enum OscError {
    #[error("could not open OSC UDP socket: {0}")]
    Socket(#[from] std::io::Error),
    #[error("OSC host must be IPv4 (got {0})")]
    NotIpv4(String),
    #[error("could not bind OSC listen port {port}: {source}")]
    Bind { port: u16, source: std::io::Error },
}

struct Inner {
    socket: Mutex<Option<UdpSocket>>,
    dest: Mutex<Option<SocketAddr>>,
    connected: AtomicBool,
    stop: AtomicBool,
    incoming: Mutex<Vec<OscMessage>>,
    last_error: Mutex<Option<String>>,
    last_sent: Mutex<String>,
    sent: Mutex<Vec<OscMessage>>,
}

/// Shared UDP session. Clone is a handle to the same socket and queues.
#[derive(Clone)]
pub struct OscSession {
    inner: Arc<Inner>,
    recv_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    dump_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Default for OscSession {
    fn default() -> Self {
        Self::new()
    }
}

impl OscSession {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                socket: Mutex::new(None),
                dest: Mutex::new(None),
                connected: AtomicBool::new(false),
                stop: AtomicBool::new(false),
                incoming: Mutex::new(Vec::new()),
                last_error: Mutex::new(None),
                last_sent: Mutex::new(String::new()),
                sent: Mutex::new(Vec::new()),
            }),
            recv_thread: Arc::new(Mutex::new(None)),
            dump_thread: Arc::new(Mutex::new(None)),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::SeqCst)
    }

    pub fn last_error(&self) -> Option<String> {
        self.inner.last_error.lock().unwrap().clone()
    }

    pub fn last_sent(&self) -> String {
        self.inner.last_sent.lock().unwrap().clone()
    }

    /// Messages this handle has sent (including before the socket is open).
    pub fn sent_messages(&self) -> Vec<OscMessage> {
        self.inner.sent.lock().unwrap().clone()
    }

    /// Drain datagrams received since the last poll.
    pub fn poll(&self) -> Vec<OscMessage> {
        std::mem::take(&mut *self.inner.incoming.lock().unwrap())
    }

    /// Bind `0.0.0.0:listen_port`, send to `host:send_port`, start recv + dump retry.
    pub fn start(&self, host: &str, send_port: u16, listen_port: u16) -> Result<(), OscError> {
        self.stop();
        self.inner.stop.store(false, Ordering::SeqCst);
        self.inner.connected.store(false, Ordering::SeqCst);

        let ipv4: Ipv4Addr = host.parse().map_err(|_| OscError::NotIpv4(host.to_string()))?;

        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        socket.set_reuse_address(true)?;
        socket.set_recv_buffer_size(RCVBUF)?;
        let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, listen_port);
        socket.bind(&bind_addr.into()).map_err(|source| OscError::Bind {
            port: listen_port,
            source,
        })?;
        socket.set_read_timeout(Some(RECV_TIMEOUT))?;

        let udp: UdpSocket = socket.into();
        let recv = udp.try_clone()?;
        recv.set_read_timeout(Some(RECV_TIMEOUT))?;
        *self.inner.socket.lock().unwrap() = Some(udp);
        *self.inner.dest.lock().unwrap() = Some(SocketAddr::V4(SocketAddrV4::new(ipv4, send_port)));

        log::info!("OSC listening :{listen_port}, sending {host}:{send_port}");

        let inner = Arc::clone(&self.inner);
        let handle = thread::Builder::new()
            .name("mixlink.osc.recv".into())
            .spawn(move || recv_loop(inner, recv))
            .map_err(OscError::Socket)?;
        *self.recv_thread.lock().unwrap() = Some(handle);

        self.request_until_connected();
        Ok(())
    }

    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        self.inner.connected.store(false, Ordering::SeqCst);
        if let Some(sock) = self.inner.socket.lock().unwrap().take() {
            let _ = sock;
        }
        *self.inner.dest.lock().unwrap() = None;
        join_thread(&self.recv_thread);
        join_thread(&self.dump_thread);
    }

    pub fn send(&self, message: OscMessage) {
        let preview = osc_preview(&message);
        {
            let mut last = self.inner.last_sent.lock().unwrap();
            if *last != preview {
                *last = preview;
            }
        }
        self.inner.sent.lock().unwrap().push(message.clone());
        let data = encode(&message);
        let sock_guard = self.inner.socket.lock().unwrap();
        let Some(sock) = sock_guard.as_ref() else {
            *self.inner.last_error.lock().unwrap() = Some("OSC socket not open".into());
            return;
        };
        let dest_guard = self.inner.dest.lock().unwrap();
        let Some(dest) = *dest_guard else {
            *self.inner.last_error.lock().unwrap() = Some("OSC socket not open".into());
            return;
        };
        if let Err(err) = sock.send_to(&data, dest) {
            *self.inner.last_error.lock().unwrap() = Some(err.to_string());
        }
    }

    pub fn send_float(&self, address: impl Into<String>, value: f32) {
        self.send(OscMessage::new(address, vec![OscValue::Float(value)]));
    }

    /// `/sendall` `/sendmix` `/sendstate` plus `/sendchan` input+output `0..=32`.
    pub fn send_dump_requests(&self) {
        self.send_float(addresses::SEND_ALL, 1.0);
        self.send_float(addresses::SEND_MIX, 1.0);
        self.send_float(addresses::SEND_STATE, 1.0);
        for bus in [Bus::Input, Bus::Output] {
            for n in 0..=32 {
                self.send_float(addresses::send_chan(bus, n), 1.0);
            }
        }
    }

    fn request_until_connected(&self) {
        join_thread(&self.dump_thread);
        let session = self.clone();
        let handle = thread::spawn(move || {
            while !session.inner.stop.load(Ordering::SeqCst) {
                if session.inner.connected.load(Ordering::SeqCst) {
                    return;
                }
                session.send_dump_requests();
                let mut slept = Duration::ZERO;
                while slept < DUMP_INTERVAL {
                    if session.inner.stop.load(Ordering::SeqCst)
                        || session.inner.connected.load(Ordering::SeqCst)
                    {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                    slept += Duration::from_millis(100);
                }
            }
        });
        *self.dump_thread.lock().unwrap() = Some(handle);
    }
}

fn recv_loop(inner: Arc<Inner>, sock: UdpSocket) {
    let mut buf = [0u8; 65_535];
    while !inner.stop.load(Ordering::SeqCst) {
        match sock.recv(&mut buf) {
            Ok(n) if n > 0 => {
                inner.connected.store(true, Ordering::SeqCst);
                let messages = decode(&buf[..n]);
                inner.incoming.lock().unwrap().extend(messages);
            }
            Ok(_) => {}
            Err(err)
                if err.kind() == ErrorKind::WouldBlock
                    || err.kind() == ErrorKind::TimedOut
                    || err.kind() == ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
}

fn join_thread(slot: &Mutex<Option<JoinHandle<()>>>) {
    if let Some(handle) = slot.lock().unwrap().take() {
        let _ = handle.join();
    }
}

fn osc_preview(message: &OscMessage) -> String {
    let args = message
        .values
        .iter()
        .map(|v| match v {
            OscValue::Float(f) => format!("{f:.3}"),
            OscValue::Int(i) => i.to_string(),
            OscValue::String(s) => s.clone(),
            OscValue::Bool(true) => "T".into(),
            OscValue::Bool(false) => "F".into(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("sent {} {args}", message.address)
}
