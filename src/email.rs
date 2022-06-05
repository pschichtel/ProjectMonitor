extern crate lettre;
extern crate lettre_email;

use lettre::{ClientSecurity, ClientTlsParameters, EmailAddress, Envelope, SendableEmail, SmtpClient, SmtpTransport, Transport};
use lettre::smtp::authentication::Credentials;
use lettre::smtp::error::SmtpResult;
use native_tls::TlsConnector;
use uuid::Uuid;

pub struct EmailContext {
    transport: SmtpTransport,
}

pub enum TransportSecurity {
    None,
    StartTls,
}

pub fn create_email_client(host: &str, port: u16, username: Option<String>, password: Option<String>, security: TransportSecurity) -> EmailContext {
    let client_security = match security {
        TransportSecurity::None => ClientSecurity::None,
        TransportSecurity::StartTls => {
            let tls_client_parameters = ClientTlsParameters {
                domain: host.to_string(),
                connector: TlsConnector::new().unwrap(),
            };
            ClientSecurity::Required(tls_client_parameters)
        },
    };

    let client = SmtpClient::new((host, port), client_security).unwrap();
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

    EmailContext {
        transport
    }
}

pub fn send_email(context: &mut EmailContext, from: EmailAddress, to: EmailAddress) -> SmtpResult {

    let email = SendableEmail::new(
        Envelope::new(
            Some(from),
            vec![to],
        )
            .unwrap(),
        Uuid::new_v4().to_string(),
        "Hello example".to_string().into_bytes(),
    );

    context.transport.send(email)
}