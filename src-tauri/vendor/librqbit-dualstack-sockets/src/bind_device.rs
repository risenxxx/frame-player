#[cfg(test)]
pub(crate) mod tests;

use crate::Error;
use std::{ffi::CString, num::NonZeroU32, str::FromStr};

#[derive(Debug, Clone)]
pub struct BindDevice {
    #[allow(unused)]
    index: NonZeroU32,
    #[allow(unused)]
    name: CString,
}

impl BindDevice {
    #[cfg(not(windows))]
    pub fn new_from_name(name: &str) -> crate::Result<Self> {
        let name = CString::new(name).map_err(|_| Error::BindDeviceInvalid)?;

        let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
        let index = NonZeroU32::new(index)
            .ok_or_else(|| Error::BindDeviceInvalidError(std::io::Error::last_os_error()))?;
        Ok(Self { index, name })
    }

    // Frame Player: upstream answers `BindDeviceNotSupported` here. Windows has
    // no `if_nametoindex` for the names people see, so the name is the
    // interface *alias* ("Wi-Fi", "Ethernet 2" — what `GetAdaptersAddresses`
    // calls `FriendlyName`), resolved through its LUID the way `netsh` does it.
    #[cfg(windows)]
    pub fn new_from_name(name: &str) -> crate::Result<Self> {
        use windows_sys::Win32::NetworkManagement::IpHelper::{
            ConvertInterfaceAliasToLuid, ConvertInterfaceLuidToIndex,
        };
        use windows_sys::Win32::NetworkManagement::Ndis::NET_LUID_LH;

        let cname = CString::new(name).map_err(|_| Error::BindDeviceInvalid)?;
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut luid: NET_LUID_LH = unsafe { std::mem::zeroed() };
        let rc = unsafe { ConvertInterfaceAliasToLuid(wide.as_ptr(), &mut luid) };
        if rc != 0 {
            return Err(Error::BindDeviceInvalidError(
                std::io::Error::from_raw_os_error(rc as i32),
            ));
        }
        let mut index = 0u32;
        let rc = unsafe { ConvertInterfaceLuidToIndex(&luid, &mut index) };
        if rc != 0 {
            return Err(Error::BindDeviceInvalidError(
                std::io::Error::from_raw_os_error(rc as i32),
            ));
        }
        let index = NonZeroU32::new(index).ok_or(Error::BindDeviceInvalid)?;
        Ok(Self { index, name: cname })
    }

    /// Frame Player: the interface's first IPv4 address, for the one client
    /// that cannot be scoped by index on Windows — reqwest's `interface()` is
    /// Unix-only, so the HTTP tracker client binds this address instead, and
    /// Windows' strong host model sends a packet from an address only through
    /// the interface that owns it.
    pub fn ipv4_addr(&self) -> Option<std::net::Ipv4Addr> {
        use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
        NetworkInterface::show()
            .ok()?
            .into_iter()
            .filter(|nic| nic.index == self.index.get())
            .flat_map(|nic| nic.addr)
            .find_map(|a| match a {
                Addr::V4(v4) if !v4.ip.is_link_local() => Some(v4.ip),
                _ => None,
            })
    }

    pub fn index(&self) -> NonZeroU32 {
        self.index
    }

    pub fn name(&self) -> &str {
        // We constructed from a string so this can't fail
        unsafe { std::str::from_utf8_unchecked(self.name.to_bytes()) }
    }

    #[cfg(target_os = "macos")]
    pub fn bind_sref(&self, sref: &socket2::Socket, is_v6: bool) -> crate::Result<()> {
        if is_v6 {
            sref.bind_device_by_index_v6(Some(self.index))
                .map_err(Error::BindDeviceSetDeviceError)
        } else {
            sref.bind_device_by_index_v4(Some(self.index))
                .map_err(Error::BindDeviceSetDeviceError)
        }
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn bind_sref(&self, sref: &socket2::Socket, _is_v6: bool) -> crate::Result<()> {
        let name = self.name.as_bytes_with_nul();
        sref.bind_device(Some(name))
            .map_err(Error::BindDeviceSetDeviceError)
    }

    // Frame Player: upstream answers `BindDeviceNotSupported` here.
    // `IP_UNICAST_IF` / `IPV6_UNICAST_IF` restrict the route lookup to one
    // interface, which is what `IP_BOUND_IF` does on macOS: a VPN's default
    // route lives on its own adapter and is simply not considered. Set before
    // `bind`/`connect`, which is where every caller in this crate puts it.
    //
    // The IPv4 value is the index in **network** byte order and the IPv6 one in
    // host order — documented, and the classic way to get this silently wrong.
    // A dual-stack socket carries IPv4 as well, and `IPPROTO_IP` options apply
    // to that half, so a v6 socket gets both; the v4 one is best effort there,
    // since a v6-only socket may refuse it and has no IPv4 half to scope.
    #[cfg(windows)]
    pub fn bind_sref(&self, sref: &socket2::Socket, is_v6: bool) -> crate::Result<()> {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{
            IP_UNICAST_IF, IPPROTO_IP, IPPROTO_IPV6, IPV6_UNICAST_IF, SOCKET, setsockopt,
        };

        let s = sref.as_raw_socket() as SOCKET;
        let set = |level: i32, opt: i32, value: u32| -> std::io::Result<()> {
            let rc = unsafe {
                setsockopt(
                    s,
                    level,
                    opt,
                    &value as *const u32 as *const u8,
                    std::mem::size_of::<u32>() as i32,
                )
            };
            if rc == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        };
        let v4 = set(IPPROTO_IP, IP_UNICAST_IF, self.index.get().to_be());
        if is_v6 {
            set(IPPROTO_IPV6, IPV6_UNICAST_IF, self.index.get())
                .map_err(Error::BindDeviceSetDeviceError)
        } else {
            v4.map_err(Error::BindDeviceSetDeviceError)
        }
    }
}

impl FromStr for BindDevice {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new_from_name(s)
    }
}
