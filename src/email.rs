extern crate lettre;
extern crate lettre_email;

use std::error::Error;
use lettre::{ClientSecurity, ClientTlsParameters, EmailAddress, Envelope, SendableEmail, SmtpClient, SmtpTransport, Transport};
use lettre::smtp::authentication::Credentials;
use lettre::smtp::error::SmtpResult;
use native_tls::TlsConnector;
use uuid::Uuid;

pub struct EmailContext {
    transport: SmtpTransport,
    from_address: EmailAddress,
    to_address: EmailAddress,
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
    from_address: EmailAddress,
    to_address: EmailAddress,
) -> Result<EmailContext, Box<dyn Error>> {
    let client_security = match security {
        TransportSecurity::None => ClientSecurity::None,
        TransportSecurity::StartTls => {
            let tls_client_parameters = ClientTlsParameters {
                domain: host.to_string(),
                connector: TlsConnector::new()?,
            };
            ClientSecurity::Required(tls_client_parameters)
        },
    };

    let client = SmtpClient::new((host, port), client_security)?;
    let transport = match (username, password) {
        (Some(user), Some(pass)) => {
            let credentials = Credentials::new(user, pass);
            client
                .credentials(credentials)
                .transport()
        },
        _ => {
            client.transport()
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
) -> SmtpResult {
    let envelope = Envelope::new(
        Some(context.from_address.clone()),
        vec![context.to_address.clone()],
    ).expect("failed to create envelope!");

    let message_id = Uuid::new_v4().to_string();
    let message = format!(r#"
From: {}
To: {}
Subject: {}

No tasks have been found in your projects, that you are not yet subscribed to.
Check the following list.

{}
"#, context.from_address, context.to_address, subject, body);

    let email = SendableEmail::new(
        envelope,
        message_id,
        message.trim().to_string().into_bytes(),
    );
    return context.transport.send(email);
}