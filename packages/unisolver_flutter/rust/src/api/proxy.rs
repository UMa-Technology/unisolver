//! The operating system's HTTP proxy settings, for `DbManager`'s downloads. Dart's `HttpClient`
//! reads only the `http_proxy` / `https_proxy` environment variables, and apps started from the
//! Dock or the Start menu have none, so a proxy set in the system settings would be ignored.

/// A manually configured proxy from the system settings
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemProxy {
    pub host: String,
    pub port: u16,
}

/// The system's manual proxy for HTTPS, else for HTTP. `None` when there is none or it comes from
/// an auto-configuration (PAC) script, and on iOS, Android and Linux, whose proxies arrive as a
/// VPN or as environment variables.
#[flutter_rust_bridge::frb(sync)]
pub fn system_proxy() -> Option<SystemProxy> {
    imp::read()
}

/// The first enabled entry that has a host and a port
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn pick(entries: &[(bool, Option<String>, Option<i64>)]) -> Option<SystemProxy> {
    entries.iter().find_map(|(on, host, port)| {
        let host = host.as_deref()?.trim();
        let port = u16::try_from((*port)?).ok().filter(|p| *p > 0)?;
        (*on && !host.is_empty()).then(|| SystemProxy {
            host: host.to_string(),
            port,
        })
    })
}

/// Windows' `ProxyServer`: `host:port` for every protocol, or one entry per protocol
/// (`http=host:port;https=host:port;…`), where HTTPS wins over HTTP
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_windows(server: &str) -> Option<SystemProxy> {
    let one = |s: &str| -> Option<SystemProxy> {
        let s = s.trim();
        let s = s.strip_prefix("http://").unwrap_or(s);
        let (host, port) = s.trim_end_matches('/').rsplit_once(':')?;
        let port = port.parse::<u16>().ok().filter(|p| *p > 0)?;
        (!host.is_empty()).then(|| SystemProxy {
            host: host.to_string(),
            port,
        })
    };
    let server = server.trim();
    if !server.contains('=') {
        return one(server);
    }
    let by = |scheme: &str| {
        server
            .split(';')
            .find_map(|e| e.trim().strip_prefix(scheme)?.strip_prefix('='))
            .and_then(one)
    };
    by("https").or_else(|| by("http"))
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{pick, SystemProxy};
    use system_configuration::core_foundation::base::CFType;
    use system_configuration::core_foundation::dictionary::CFDictionary;
    use system_configuration::core_foundation::number::CFNumber;
    use system_configuration::core_foundation::string::{CFString, CFStringRef};
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;
    use system_configuration::sys::schema_definitions::{
        kSCPropNetProxiesHTTPEnable, kSCPropNetProxiesHTTPPort, kSCPropNetProxiesHTTPProxy,
        kSCPropNetProxiesHTTPSEnable, kSCPropNetProxiesHTTPSPort, kSCPropNetProxiesHTTPSProxy,
    };

    fn entry(
        d: &CFDictionary<CFString, CFType>,
        enable: CFStringRef,
        host: CFStringRef,
        port: CFStringRef,
    ) -> (bool, Option<String>, Option<i64>) {
        let num = |k: CFStringRef| {
            d.find(k)
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|n| n.to_i64())
        };
        let host = d
            .find(host)
            .and_then(|v| v.downcast::<CFString>())
            .map(|s| s.to_string());
        (num(enable) == Some(1), host, num(port))
    }

    pub(super) fn read() -> Option<SystemProxy> {
        let d = SCDynamicStoreBuilder::new("unisolver")
            .build()
            .get_proxies()?;
        // SAFETY: the schema keys are immutable CFString constants exported by SystemConfiguration
        let (https, http) = unsafe {
            (
                entry(
                    &d,
                    kSCPropNetProxiesHTTPSEnable,
                    kSCPropNetProxiesHTTPSProxy,
                    kSCPropNetProxiesHTTPSPort,
                ),
                entry(
                    &d,
                    kSCPropNetProxiesHTTPEnable,
                    kSCPropNetProxiesHTTPProxy,
                    kSCPropNetProxiesHTTPPort,
                ),
            )
        };
        pick(&[https, http])
    }
}

#[cfg(windows)]
mod imp {
    use super::{parse_windows, SystemProxy};
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn dword(name: &str) -> Option<u32> {
        let (key, value) = (wide(KEY), wide(name));
        let mut v = 0u32;
        let mut len = 4u32;
        // SAFETY: NUL-terminated key and value names; the out buffer is a u32 of `len` bytes
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                &mut v as *mut u32 as *mut _,
                &mut len,
            )
        };
        (r == 0).then_some(v)
    }

    fn string(name: &str) -> Option<String> {
        let (key, value) = (wide(KEY), wide(name));
        let mut len = 0u32;
        // SAFETY: a null buffer asks for the size in bytes
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut len,
            )
        };
        if r != 0 {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        // SAFETY: the buffer holds `len` bytes
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut _,
                &mut len,
            )
        };
        if r != 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }

    pub(super) fn read() -> Option<SystemProxy> {
        if dword("ProxyEnable")? == 0 {
            return None;
        }
        parse_windows(&string("ProxyServer")?)
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    pub(super) fn read() -> Option<super::SystemProxy> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(host: &str, port: u16) -> Option<SystemProxy> {
        Some(SystemProxy {
            host: host.to_string(),
            port,
        })
    }

    #[test]
    fn the_first_enabled_entry_with_host_and_port_wins() {
        let https = (true, Some("127.0.0.1".to_string()), Some(7890));
        let http = (true, Some("10.0.0.2".to_string()), Some(8080));
        assert_eq!(pick(&[https.clone(), http.clone()]), p("127.0.0.1", 7890));
        assert_eq!(
            pick(&[(false, https.1.clone(), https.2), http]),
            p("10.0.0.2", 8080)
        );
        assert_eq!(
            pick(&[
                (true, Some(" ".into()), Some(1)),
                (true, Some("h".into()), None)
            ]),
            None
        );
        assert_eq!(pick(&[(true, Some("h".into()), Some(70000))]), None);
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn windows_settings_for_every_protocol_or_per_protocol() {
        assert_eq!(parse_windows("127.0.0.1:7890"), p("127.0.0.1", 7890));
        assert_eq!(
            parse_windows("http=10.0.0.2:8080;https=10.0.0.3:8443"),
            p("10.0.0.3", 8443)
        );
        assert_eq!(
            parse_windows("http=10.0.0.2:8080;ftp=x:21"),
            p("10.0.0.2", 8080)
        );
        assert_eq!(parse_windows("socks=127.0.0.1:1080"), None);
        assert_eq!(
            parse_windows("http://proxy.lan:3128/"),
            p("proxy.lan", 3128)
        );
        assert_eq!(parse_windows(""), None);
        assert_eq!(parse_windows("nohost"), None);
    }

    #[test]
    fn reading_the_settings_never_panics() {
        let _ = system_proxy();
    }
}
