// Copyright 2021 Jeremy Wall
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::{
    convert::{Into, TryFrom, TryInto},
    time::Duration,
    ffi::CStr,
};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::packet::{Icmpv4Packet, Icmpv6Packet};

fn ip_to_socket(ip: &IpAddr) -> SocketAddr {
    SocketAddr::new(*ip, 0)
}

/// Trait for an IcmpSocket implemented by Icmpv4Socket and Icmpv6Socket.
pub trait IcmpSocket {
    /// The type of address this socket operates on.
    type AddrType;
    /// The type of packet this socket handles.
    type PacketType;

    /// Sets the timeout on the socket for rcv_from. A value of 0 will cause
    /// rcv_from to block.
    fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()>;

    /// Sets the ttl for packets sent on this socket. Controls the number of
    /// hops the packet will be allowed to traverse.
    fn set_max_hops(&mut self, hops: u32);

    /// Binds this socket to an address.
    fn bind<A: Into<Self::AddrType>>(&mut self, addr: A) -> std::io::Result<()>;

    /// Sends the packet to the given destination.
    fn send_to(&mut self, dest: Self::AddrType, packet: Self::PacketType) -> std::io::Result<()>;

    /// Receive a packet on this socket.
    fn rcv_from(&mut self) -> std::io::Result<(Self::PacketType, SockAddr)>;

    #[cfg(target_os = "linux")]
    /// Bind to specific interface
    fn bind_device(&mut self, interface_name: Option<&CStr>) -> std::io::Result<()>;
}

/// Options for this socket.
pub struct Opts {
    hops: u32,
}

/// An ICMPv4 socket.
pub struct IcmpSocket4 {
    bound_to: Option<Ipv4Addr>,
    buf: Vec<u8>,
    inner: Socket,
    opts: Opts,
}

impl IcmpSocket4 {
    /// Construct a new socket. The socket must be bound to an address using `bind_to`
    /// before it can be used to send and receive packets.
    pub fn new() -> std::io::Result<Self> {
        let socket = Socket::new(Domain::ipv4(), Type::raw(), Some(Protocol::icmpv4()))?;
        socket.set_recv_buffer_size(512)?;
        Ok(Self {
            bound_to: None,
            inner: socket,
            buf: vec![0; 512],
            opts: Opts { hops: 50 },
        })
    }
}

impl IcmpSocket for IcmpSocket4 {
    type AddrType = Ipv4Addr;
    type PacketType = Icmpv4Packet;

    fn set_max_hops(&mut self, hops: u32) {
        self.opts.hops = hops;
    }

    fn bind<A: Into<Self::AddrType>>(&mut self, addr: A) -> std::io::Result<()> {
        let addr = addr.into();
        self.bound_to = Some(addr.clone());
        let sock = ip_to_socket(&IpAddr::V4(addr));
        self.inner.bind(&(sock.into()))?;
        Ok(())
    }

    fn send_to(&mut self, dest: Self::AddrType, packet: Self::PacketType) -> std::io::Result<()> {
        let dest = ip_to_socket(&IpAddr::V4(dest));
        self.inner.set_ttl(self.opts.hops)?;
        self.inner
            .send_to(&packet.with_checksum().get_bytes(true), &(dest.into()))?;
        Ok(())
    }

    fn rcv_from(&mut self) -> std::io::Result<(Self::PacketType, SockAddr)> {
        self.inner.set_read_timeout(None)?;
        let (read_count, addr) = self.inner.recv_from(&mut self.buf)?;
        Ok((self.buf[0..read_count].try_into()?, addr))
    }

    fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()> {
        self.inner.set_read_timeout(Some(timeout))
    }

    #[cfg(target_os = "linux")]
    fn bind_device(&mut self, interface_name: Option<&CStr>) -> std::io::Result<()> {
        self.inner.bind_device(interface_name)
    }
}

/// An Icmpv6 socket.
pub struct IcmpSocket6 {
    bound_to: Option<Ipv6Addr>,
    inner: Socket,
    buf: Vec<u8>,
    opts: Opts,
}

impl IcmpSocket6 {
    /// Construct a new socket. The socket must be bound to an address using `bind_to`
    /// before it can be used to send and receive packets.
    pub fn new() -> std::io::Result<Self> {
        let socket = Socket::new(Domain::ipv6(), Type::raw(), Some(Protocol::icmpv6()))?;
        socket.set_recv_buffer_size(512)?;
        Ok(Self {
            bound_to: None,
            inner: socket,
            buf: vec![0; 512],
            opts: Opts { hops: 50 },
        })
    }
}

impl IcmpSocket for IcmpSocket6 {
    type AddrType = Ipv6Addr;
    type PacketType = Icmpv6Packet;

    fn set_max_hops(&mut self, hops: u32) {
        self.opts.hops = hops;
    }

    fn bind<A: Into<Self::AddrType>>(&mut self, addr: A) -> std::io::Result<()> {
        let addr = addr.into();
        self.bound_to = Some(addr.clone());
        let sock = ip_to_socket(&IpAddr::V6(addr));
        self.inner.bind(&(sock.into()))?;
        Ok(())
    }

    fn send_to(
        &mut self,
        dest: Self::AddrType,
        mut packet: Self::PacketType,
    ) -> std::io::Result<()> {
        let source = match self.bound_to {
            Some(ref addr) => addr,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Socket not bound to an address",
                ))
            }
        };
        packet = packet.with_checksum(source, &dest);
        let dest = ip_to_socket(&IpAddr::V6(dest));
        self.inner.set_unicast_hops_v6(self.opts.hops)?;
        let pkt = packet.get_bytes(true);
        self.inner.send_to(&pkt, &(dest.into()))?;
        Ok(())
    }

    fn rcv_from(&mut self) -> std::io::Result<(Self::PacketType, SockAddr)> {
        self.inner.set_read_timeout(None)?;
        let (read_count, addr) = self.inner.recv_from(&mut self.buf)?;
        Ok((self.buf[0..read_count].try_into()?, addr))
    }

    fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()> {
        self.inner.set_read_timeout(Some(timeout))
    }

    #[cfg(target_os = "linux")]
    fn bind_device(&mut self, interface_name: Option<&CStr>) -> std::io::Result<()> {
        self.inner.bind_device(interface_name)
    }
}


impl TryFrom<Ipv4Addr> for IcmpSocket4 {
    type Error = std::io::Error;

    fn try_from(addr: Ipv4Addr) -> Result<Self, Self::Error> {
        let mut sock = IcmpSocket4::new()?;
        sock.bind(addr)?;
        Ok(sock)
    }
}

impl TryFrom<Ipv6Addr> for IcmpSocket6 {
    type Error = std::io::Error;

    fn try_from(addr: Ipv6Addr) -> Result<Self, Self::Error> {
        let mut sock = IcmpSocket6::new()?;
        sock.bind(addr)?;
        Ok(sock)
    }
}
