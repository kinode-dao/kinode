use crate::kernel::component::uq_process::types as wit;
use crate::types as t;

//
// conversions between wit types and kernel types (annoying)
//

pub fn en_wit_process_id(process_id: t::ProcessId) -> wit::ProcessId {
    match process_id {
        t::ProcessId::Id(id) => wit::ProcessId::Id(id),
        t::ProcessId::Name(name) => wit::ProcessId::Name(name),
    }
}

pub fn de_wit_process_id(process_id: wit::ProcessId) -> t::ProcessId {
    match process_id {
        wit::ProcessId::Id(id) => t::ProcessId::Id(id),
        wit::ProcessId::Name(name) => t::ProcessId::Name(name),
    }
}

pub fn en_wit_address(address: t::Address) -> wit::Address {
    wit::Address {
        node: address.node,
        process: match address.process {
            t::ProcessId::Id(id) => wit::ProcessId::Id(id),
            t::ProcessId::Name(name) => wit::ProcessId::Name(name),
        },
    }
}

pub fn de_wit_address(wit: wit::Address) -> t::Address {
    t::Address {
        node: wit.node,
        process: match wit.process {
            wit::ProcessId::Id(id) => t::ProcessId::Id(id),
            wit::ProcessId::Name(name) => t::ProcessId::Name(name),
        },
    }
}

pub fn en_wit_message(message: t::Message) -> wit::Message {
    match message {
        t::Message::Request(request) => wit::Message::Request(en_wit_request(request)),
        t::Message::Response((response, context)) => {
            wit::Message::Response((en_wit_response(response), context))
        }
    }
}

pub fn de_wit_request(wit: wit::Request) -> t::Request {
    t::Request {
        inherit: wit.inherit,
        expects_response: wit.expects_response,
        ipc: wit.ipc,
        metadata: wit.metadata,
    }
}

pub fn en_wit_request(request: t::Request) -> wit::Request {
    wit::Request {
        inherit: request.inherit,
        expects_response: request.expects_response,
        ipc: request.ipc,
        metadata: request.metadata,
    }
}

pub fn de_wit_response(wit: wit::Response) -> t::Response {
    t::Response {
        ipc: wit.ipc,
        metadata: wit.metadata,
    }
}

pub fn en_wit_response(response: t::Response) -> wit::Response {
    wit::Response {
        ipc: response.ipc,
        metadata: response.metadata,
    }
}

pub fn en_wit_send_error(error: t::SendError) -> wit::SendError {
    wit::SendError {
        kind: en_wit_send_error_kind(error.kind),
        message: en_wit_message(error.message),
        payload: en_wit_payload(error.payload),
    }
}

pub fn en_wit_send_error_kind(kind: t::SendErrorKind) -> wit::SendErrorKind {
    match kind {
        t::SendErrorKind::Offline => wit::SendErrorKind::Offline,
        t::SendErrorKind::Timeout => wit::SendErrorKind::Timeout,
    }
}

pub fn de_wit_payload(wit: Option<wit::Payload>) -> Option<t::Payload> {
    match wit {
        None => None,
        Some(wit) => Some(t::Payload {
            mime: wit.mime,
            bytes: wit.bytes,
        }),
    }
}

pub fn en_wit_payload(payload: Option<t::Payload>) -> Option<wit::Payload> {
    match payload {
        None => None,
        Some(payload) => Some(wit::Payload {
            mime: payload.mime,
            bytes: payload.bytes,
        }),
    }
}

pub fn de_wit_signed_capability(wit: wit::SignedCapability) -> t::SignedCapability {
    t::SignedCapability {
        issuer: de_wit_address(wit.issuer),
        params: wit.params,
        signature: wit.signature,
    }
}

pub fn en_wit_signed_capability(cap: t::SignedCapability) -> wit::SignedCapability {
    wit::SignedCapability {
        issuer: en_wit_address(cap.issuer),
        params: cap.params,
        signature: cap.signature,
    }
}

pub fn de_wit_on_panic(wit: wit::OnPanic) -> t::OnPanic {
    match wit {
        wit::OnPanic::None => t::OnPanic::None,
        wit::OnPanic::Restart => t::OnPanic::Restart,
        wit::OnPanic::Requests(reqs) => t::OnPanic::Requests(
            reqs.into_iter()
                .map(|(address, request, payload)| {
                    (
                        de_wit_address(address),
                        de_wit_request(request),
                        de_wit_payload(payload),
                    )
                })
                .collect(),
        ),
    }
}
