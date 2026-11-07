use super::*;

pub(super) async fn select(
    task: &DeliveryTask,
    target: AddressHash,
    method: Method,
) -> Option<Option<Context>> {
    let selected = if method == Method::Propagated {
        match while_ready(
            task,
            propagation::propagation_link_context(
                &task.outbound_propagation_link,
                &target.to_hex_string(),
            ),
        )
        .await?
        {
            Some(link) => Some(link),
            None => while_ready(task, task.transport.delivery_link(&target)).await?,
        }
    } else {
        while_ready(task, task.transport.delivery_link(&target)).await?
    };
    let context = if let Some(link) = selected {
        let initiator = while_ready(task, async { link.lock().await.is_initiator() }).await?;
        if method == Method::Propagated && !initiator {
            None
        } else {
            Some(Context {
                link,
                peer: target.to_hex_string(),
                method,
                identify: initiator && method != Method::Propagated,
                signal: None,
            })
        }
    } else {
        None
    };
    Some(context)
}

pub(super) async fn open(
    task: &DeliveryTask,
    target: AddressHash,
    identity: rns_transport::identity::Identity,
    method: Method,
) -> Option<Context> {
    let descriptor = DestinationDesc {
        identity,
        address_hash: target,
        name: DestinationName::new(
            "lxmf",
            if method == Method::Propagated { "propagation" } else { "delivery" },
        ),
    };
    let mut signal = None;
    let link = if method == Method::Propagated {
        while_ready(
            task,
            propagation::propagation_link_for_node(
                task.transport.as_ref(),
                &task.outbound_propagation_link,
                &target.to_hex_string(),
                descriptor,
                |link| {
                    signal =
                        link.observe_msgpack_signal(propagation::PROPAGATION_INVALID_STAMP_SIGNAL);
                },
            ),
        )
        .await?
    } else {
        while_ready(task, task.transport.link(descriptor)).await?
    };
    Some(Context {
        link,
        peer: target.to_hex_string(),
        method,
        identify: method != Method::Propagated,
        signal,
    })
}
