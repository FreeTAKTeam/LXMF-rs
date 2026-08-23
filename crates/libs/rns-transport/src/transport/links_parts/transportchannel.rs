impl TransportChannel {
    async fn find_link(&self) -> Option<Arc<Mutex<Link>>> {
        let (out_links, in_link) = {
            let handler = self.handler.lock().await;
            (
                handler.out_links.values().cloned().collect::<Vec<_>>(),
                handler.in_links.get(&self.link_id).cloned(),
            )
        };

        if let Some(link) = in_link {
            return Some(link);
        }

        for link in out_links {
            if *link.lock().await.id() == self.link_id {
                return Some(link);
            }
        }

        None
    }

    pub fn link_id(&self) -> AddressHash {
        self.link_id
    }

    pub async fn mdu(&self) -> Result<usize, crate::channel::ChannelError> {
        const CHANNEL_ENVELOPE_LENGTH: usize = 6;
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let channel_mdu = link
            .lock()
            .await
            .link_mdu()
            .saturating_sub(CHANNEL_ENVELOPE_LENGTH)
            .min(u16::MAX as usize);
        Ok(channel_mdu)
    }

    pub async fn send(
        &self,
        msg_type: u16,
        payload: Vec<u8>,
    ) -> Result<u16, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;

        let (sequence, iface, packet) = {
            let mut link = link.lock().await;
            let iface = link.ingress_iface().ok_or(crate::channel::ChannelError::LinkNotReady)?;
            let (sequence, packet) = link.send_channel_message(msg_type, payload)?;
            (sequence, iface, packet)
        };

        let dispatch = self
            .handler
            .lock()
            .await
            .send(TxMessage { tx_type: TxMessageType::Direct(iface), packet })
            .await;
        if dispatch.sent_ifaces == 0 {
            link.lock().await.mark_channel_failed(sequence);
            return Err(crate::channel::ChannelError::LinkNotReady);
        }

        Ok(sequence)
    }

    pub async fn open(&self) -> Result<(), crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        link.lock().await.open_channel();
        Ok(())
    }

    pub async fn close(&self) -> Result<(), crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        link.lock().await.close_channel();
        Ok(())
    }

    pub async fn is_ready_to_send(&self) -> Result<bool, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let ready = link.lock().await.channel_ready_to_send();
        Ok(ready)
    }

    pub async fn close_wait_hint(&self) -> Result<Duration, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let hint = link.lock().await.channel_close_wait_hint();
        Ok(hint)
    }
    pub async fn send_typed<M: crate::channel::TypedMessage>(
        &self,
        message: &M,
    ) -> Result<u16, crate::channel::ChannelError> {
        self.send(M::MSG_TYPE, message.encode()).await
    }

    pub async fn register_handler<F>(
        &self,
        msg_type: u16,
        handler: F,
    ) -> Result<crate::channel::HandlerId, crate::channel::ChannelError>
    where
        F: FnMut(crate::channel::Envelope) -> bool + Send + 'static,
    {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let handler_id = link.lock().await.register_channel_handler(msg_type, handler);
        Ok(handler_id)
    }

    pub async fn register_typed_handler<M, F>(
        &self,
        mut handler: F,
    ) -> Result<crate::channel::HandlerId, crate::channel::ChannelError>
    where
        M: crate::channel::TypedMessage,
        F: FnMut(M) -> bool + Send + 'static,
    {
        crate::channel::validate_typed_message_type::<M>()?;
        self.register_handler(M::MSG_TYPE, move |envelope| match M::decode(&envelope.payload) {
            Ok(message) => handler(message),
            Err(_) => false,
        })
        .await
    }

    pub async fn remove_handler(
        &self,
        handler_id: crate::channel::HandlerId,
    ) -> Result<bool, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let removed = link.lock().await.remove_channel_handler(handler_id);
        Ok(removed)
    }

    pub async fn message_state(
        &self,
        sequence: u16,
    ) -> Result<crate::channel::MessageState, crate::channel::ChannelError> {
        let link = self.find_link().await.ok_or(crate::channel::ChannelError::LinkNotReady)?;
        let state = link.lock().await.channel_state(sequence);
        Ok(state)
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
