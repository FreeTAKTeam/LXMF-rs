impl AutoDiscoverySocketBindTarget {
    fn unicast(listener: &AutoDiscoveryListenerBinding) -> Self {
        let (bind_host, scope_ifname) =
            bind_host_and_scope(&listener.unicast_bind_address, &listener.ifname);
        Self {
            kind: AutoDiscoverySocketKind::Unicast,
            ifname: listener.ifname.clone(),
            bind_host,
            bind_port: listener.unicast_bind_port,
            scope_ifname,
            multicast_group_host: None,
        }
    }

    fn multicast(listener: &AutoDiscoveryListenerBinding) -> Self {
        let (bind_host, scope_ifname) =
            bind_host_and_scope(&listener.multicast_bind_address, &listener.ifname);
        Self {
            kind: AutoDiscoverySocketKind::Multicast,
            ifname: listener.ifname.clone(),
            bind_host,
            bind_port: listener.multicast_bind_port,
            scope_ifname,
            multicast_group_host: Some(listener.multicast_group_address.clone()),
        }
    }

    pub(crate) fn display_bind_addr(&self) -> String {
        let host = if let Some(scope_ifname) = &self.scope_ifname {
            format!("{}%{scope_ifname}", self.bind_host)
        } else {
            self.bind_host.clone()
        };
        socket_target(&host, self.bind_port)
    }

    fn resolve_bind_addr(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<SocketAddr, String> {
        let ip = self
            .bind_host
            .parse::<IpAddr>()
            .map_err(|err| format!("parse auto discovery bind host {}: {err}", self.bind_host))?;
        match (ip, self.scope_ifname.as_deref()) {
            (IpAddr::V6(host), Some(ifname)) => {
                let scope_id = scope_id_for_ifname(ifname).map_err(|err| {
                    format!("resolve auto discovery scope id for interface {ifname}: {err}")
                })?;
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.bind_port, 0, scope_id)))
            }
            (IpAddr::V6(host), None) => {
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.bind_port, 0, 0)))
            }
            (IpAddr::V4(host), None) => Ok(SocketAddr::from((host, self.bind_port))),
            (IpAddr::V4(_), Some(ifname)) => Err(format!(
                "auto discovery IPv4 bind host {} cannot use scope interface {ifname}",
                self.bind_host
            )),
        }
    }

    fn resolve_multicast_bind(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<AutoResolvedMulticastDiscoveryBind, String> {
        if self.kind != AutoDiscoverySocketKind::Multicast {
            return Err("auto discovery multicast bind resolver requires a multicast target".into());
        }
        let group_host = self.multicast_group_host.as_ref().ok_or_else(|| {
            "auto discovery multicast target is missing multicast group".to_string()
        })?;
        let group_ip = group_host.parse::<IpAddr>().map_err(|err| {
            format!("parse auto discovery multicast group host {group_host}: {err}")
        })?;
        if !group_ip.is_multicast() {
            return Err(format!("auto discovery group host {group_host} is not multicast"));
        }
        let join_scope_ifname = match group_ip {
            IpAddr::V6(_) if is_link_scope_ipv6_multicast(group_host) => {
                Some(self.scope_ifname.as_deref().unwrap_or(self.ifname.as_str()))
            }
            IpAddr::V6(_) => self.scope_ifname.as_deref(),
            IpAddr::V4(_) => None,
        };
        let multicast_scope_id = if let Some(ifname) = join_scope_ifname {
            scope_id_for_ifname(ifname).map_err(|err| {
                format!("resolve auto discovery multicast scope id for interface {ifname}: {err}")
            })?
        } else {
            0
        };
        let multicast_group_addr = match group_ip {
            IpAddr::V6(group) => {
                SocketAddr::V6(SocketAddrV6::new(group, self.bind_port, 0, multicast_scope_id))
            }
            IpAddr::V4(group) => SocketAddr::from((group, self.bind_port)),
        };
        let bind_addr = self.resolve_multicast_socket_bind_addr(&mut scope_id_for_ifname)?;
        Ok(AutoResolvedMulticastDiscoveryBind {
            bind_addr,
            multicast_group_addr,
            multicast_scope_id,
        })
    }

    fn resolve_multicast_socket_bind_addr(
        &self,
        scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<SocketAddr, String> {
        let bind_ip = self.bind_host.parse::<IpAddr>().map_err(|err| {
            format!("parse auto discovery multicast bind host {}: {err}", self.bind_host)
        })?;
        if bind_ip.is_multicast() {
            return Ok(match bind_ip {
                IpAddr::V6(_) => SocketAddr::V6(SocketAddrV6::new(
                    std::net::Ipv6Addr::UNSPECIFIED,
                    self.bind_port,
                    0,
                    0,
                )),
                IpAddr::V4(_) => {
                    SocketAddr::from((std::net::Ipv4Addr::UNSPECIFIED, self.bind_port))
                }
            });
        }
        self.resolve_bind_addr(scope_id_for_ifname)
    }
}

impl AutoDataSocketBindTarget {
    fn from_listener(listener: &AutoDataListenerBinding) -> Self {
        let (bind_host, scope_ifname) =
            bind_host_and_scope(&listener.bind_address, &listener.ifname);
        Self {
            ifname: listener.ifname.clone(),
            bind_host,
            bind_port: listener.bind_port,
            scope_ifname,
        }
    }

    pub(crate) fn display_bind_addr(&self) -> String {
        let host = if let Some(scope_ifname) = &self.scope_ifname {
            format!("{}%{scope_ifname}", self.bind_host)
        } else {
            self.bind_host.clone()
        };
        socket_target(&host, self.bind_port)
    }

    fn resolve_bind_addr(
        &self,
        mut scope_id_for_ifname: impl FnMut(&str) -> Result<u32, String>,
    ) -> Result<SocketAddr, String> {
        let ip = self
            .bind_host
            .parse::<IpAddr>()
            .map_err(|err| format!("parse auto peer data bind host {}: {err}", self.bind_host))?;
        match (ip, self.scope_ifname.as_deref()) {
            (IpAddr::V6(host), Some(ifname)) => {
                let scope_id = scope_id_for_ifname(ifname).map_err(|err| {
                    format!("resolve auto peer data scope id for interface {ifname}: {err}")
                })?;
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.bind_port, 0, scope_id)))
            }
            (IpAddr::V6(host), None) => {
                Ok(SocketAddr::V6(SocketAddrV6::new(host, self.bind_port, 0, 0)))
            }
            (IpAddr::V4(host), None) => Ok(SocketAddr::from((host, self.bind_port))),
            (IpAddr::V4(_), Some(ifname)) => Err(format!(
                "auto peer data IPv4 bind host {} cannot use scope interface {ifname}",
                self.bind_host
            )),
        }
    }
}
