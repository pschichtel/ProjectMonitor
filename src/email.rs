extern crate lettre;

use std::error::Error as StdError;
use lettre::{Address, SmtpTransport, Transport};
use lettre::transport::smtp::authentication::Credentials;
use lettre::address::Envelope;
use lettre::message::{Mailbox, MessageBuilder};
use lettre::transport::smtp::client::{Tls, TlsParametersBuilder};
use lettre::transport::smtp::Error;
use lettre::transport::smtp::response::Response;

pub struct EmailContext {
    transport: SmtpTransport,
    from_address: Address,
    to_address: Address,
}

pub enum TransportSecurity {
    None,
    StartTls,
}

pub fn create_email_client(
    host: &str,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    security: TransportSecurity,
    from_address: Address,
    to_address: Address,
) -> Result<EmailContext, Box<dyn StdError>> {
    let client_security = match security {
        TransportSecurity::None => Tls::None,
        TransportSecurity::StartTls => {
            let tls_client_parameters =
                TlsParametersBuilder::new(host.to_string()).build_rustls()?;
            Tls::Required(tls_client_parameters)
        },
    };

    let transport_builder = SmtpTransport::builder_dangerous(host)
        .port(port)
        .tls(client_security);
    let transport = match (username, password) {
        (Some(user), Some(pass)) => {
            let credentials = Credentials::new(user, pass);
            transport_builder
                .credentials(credentials)
                .build()
        },
        _ => {
            transport_builder.build()
        }
    };

    let context = EmailContext {
        transport,
        from_address,
        to_address,
    };

    Ok(context)
}

pub fn send_email(
    context: &mut EmailContext,
    subject: &str,
    body: &str,
) -> Result<Response, Error> {
    let envelope = Envelope::new(
        Some(context.from_address.to_owned()),
        vec![context.to_address.to_owned()],
    ).expect("failed to create envelope!");

    let message = format!(r#"
From: {}
To: {}
Subject: {}

No tasks have been found in your projects, that you are not yet subscribed to.
Check the following list.

{}
"#, context.from_address, context.to_address, subject, body);

    let message = MessageBuilder::new()
        .message_id(None)
        .subject(subject)
        .envelope(envelope)
        .from(Mailbox::new(None, context.from_address.to_owned()))
        .to(Mailbox::new(None, context.to_address.to_owned()))
        .body(message.trim().to_string().into_bytes())
        .unwrap();

    context.transport.send(&message)
}