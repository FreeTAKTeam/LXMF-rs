#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestedDeliveryMethod {
    Opportunistic,
    Direct,
    Propagated,
    Paper,
}

impl RequestedDeliveryMethod {
    pub(crate) fn parse(method: Option<&str>) -> Result<Self, std::io::Error> {
        let normalized = method.map(str::trim).unwrap_or_default().to_ascii_lowercase();
        match normalized.as_str() {
            "" | "direct" => Ok(Self::Direct),
            "opportunistic" => Ok(Self::Opportunistic),
            "propagated" => Ok(Self::Propagated),
            "paper" => Ok(Self::Paper),
            other => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported delivery method '{other}'"),
            )),
        }
    }
}

pub(crate) fn validate_delivery_request(
    method: RequestedDeliveryMethod,
    propagation_node: Option<&str>,
) -> Result<(), std::io::Error> {
    match method {
        RequestedDeliveryMethod::Propagated => {
            if propagation_node.is_none() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no outbound propagation node selected",
                ));
            }
            Ok(())
        }
        RequestedDeliveryMethod::Paper
        | RequestedDeliveryMethod::Opportunistic
        | RequestedDeliveryMethod::Direct => Ok(()),
    }
}
