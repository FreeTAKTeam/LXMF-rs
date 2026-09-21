#[derive(Debug)]
enum ManagementAction {
    Rates,
    DropPath { destination: String },
    DropAnnounces,
    DropVia { transport: String },
    ListBlackholed,
    Blackhole { identity: String, until: Option<f64>, reason: Option<String> },
    Unblackhole { identity: String },
}

fn run_management(
    cli: &Cli,
    output: &mut dyn Write,
    action: ManagementAction,
) -> io::Result<()> {
    let (method, params, label) = match action {
        ManagementAction::Rates => ("get_rate_table", None, "rates"),
        ManagementAction::DropPath { destination } => (
            "drop_path",
            Some(json!({ "destination": destination })),
            "drop",
        ),
        ManagementAction::DropAnnounces => ("drop_announce_queues", None, "drop-announces"),
        ManagementAction::DropVia { transport } => (
            "drop_all_via",
            Some(json!({ "destination": transport })),
            "drop-via",
        ),
        ManagementAction::ListBlackholed => ("get_blackholed_identities", None, "blackholed"),
        ManagementAction::Blackhole { identity, until, reason } => (
            "blackhole_identity",
            Some(json!({ "identity": identity, "until": until, "reason": reason })),
            "blackhole",
        ),
        ManagementAction::Unblackhole { identity } => (
            "unblackhole_identity",
            Some(json!({ "identity": identity })),
            "unblackhole",
        ),
    };

    let response = rpc_call(cli, 0, method, params)?;
    let result = ensure_rpc_ok(response, method)?.unwrap_or(Value::Null);
    if cli.json {
        writeln!(output, "{}", serde_json::to_string_pretty(&result)?)?;
    } else {
        writeln!(output, "{label}={result}")?;
    }
    Ok(())
}

impl Cli {
    fn management_action(&self) -> io::Result<Option<ManagementAction>> {
        let selected = self.rates as u8
            + self.drop as u8
            + self.drop_announces as u8
            + self.drop_via.is_some() as u8
            + self.blackholed as u8
            + self.blackhole.is_some() as u8
            + self.unblackhole.is_some() as u8;
        if selected > 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "management options cannot be combined",
            ));
        }

        if self.rates {
            return self.management_without_destination(ManagementAction::Rates);
        }
        if self.drop {
            let destination = self.destination_hash.clone().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "--drop requires a destination hash")
            })?;
            return self.management_without_auxiliary_options(ManagementAction::DropPath {
                destination,
            });
        }
        if self.drop_announces {
            return self.management_without_destination(ManagementAction::DropAnnounces);
        }
        if let Some(transport) = self.drop_via.clone() {
            return self.management_without_destination(ManagementAction::DropVia { transport });
        }
        if self.blackholed {
            return self.management_without_destination(ManagementAction::ListBlackholed);
        }
        if let Some(identity) = self.blackhole.clone() {
            let until = self.blackhole_expiry()?;
            return self.management_without_destination(ManagementAction::Blackhole {
                identity,
                until,
                reason: self.reason.clone(),
            });
        }
        if let Some(identity) = self.unblackhole.clone() {
            if self.duration.is_some() || self.reason.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--duration and --reason require --blackhole",
                ));
            }
            return self.management_without_destination(ManagementAction::Unblackhole { identity });
        }
        if self.duration.is_some() || self.reason.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--duration and --reason require --blackhole",
            ));
        }
        Ok(None)
    }

    fn management_without_destination(
        &self,
        action: ManagementAction,
    ) -> io::Result<Option<ManagementAction>> {
        if self.destination_hash.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a positional destination cannot be combined with this management option",
            ));
        }
        if self.on_iface.is_some() || self.tag_hex.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path scope cannot be combined with this management option",
            ));
        }
        if !matches!(action, ManagementAction::Blackhole { .. })
            && (self.duration.is_some() || self.reason.is_some())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--duration and --reason require --blackhole",
            ));
        }
        Ok(Some(action))
    }

    fn management_without_auxiliary_options(
        &self,
        action: ManagementAction,
    ) -> io::Result<Option<ManagementAction>> {
        if self.on_iface.is_some()
            || self.tag_hex.is_some()
            || self.duration.is_some()
            || self.reason.is_some()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path scope and blackhole options cannot be combined with --drop",
            ));
        }
        Ok(Some(action))
    }

    fn blackhole_expiry(&self) -> io::Result<Option<f64>> {
        let Some(hours) = self.duration else {
            return Ok(None);
        };
        if !hours.is_finite() || hours < 0.0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--duration must be a finite non-negative number of hours",
            ));
        }
        let seconds = hours * 3600.0;
        if !seconds.is_finite() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "--duration is too large",
            ));
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs_f64();
        Ok(Some(now + seconds))
    }
}
