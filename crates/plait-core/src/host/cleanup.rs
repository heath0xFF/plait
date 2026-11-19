//! Additive checkout cleanup reads and explicit, token-bound confirmation.
use super::{Client, Error};
use crate::{lease::cleanup::Cleanup, ownership::CheckoutKey};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request<'a> {
    CheckoutCleanup {
        checkout: &'a PathBuf,
    },
    ResolveCheckoutCleanup {
        checkout: &'a PathBuf,
        token: &'a str,
        confirm_stopped: bool,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    checkout: PathBuf,
    cleanup: Option<Cleanup>,
}

pub fn read(
    client: &Client,
    identity: &CheckoutKey,
    confirm: Option<&str>,
) -> Result<Option<Cleanup>, Error> {
    identity.verify().map_err(Error::Protocol)?;
    let request = match confirm {
        Some(token) => Request::ResolveCheckoutCleanup {
            checkout: &identity.root,
            token,
            confirm_stopped: true,
        },
        None => Request::CheckoutCleanup {
            checkout: &identity.root,
        },
    };
    let reply: Reply = client.send(&request)?;
    identity.verify().map_err(Error::Protocol)?;
    if reply.checkout != identity.root
        || reply.cleanup.as_ref().is_some_and(|state| {
            state.token.is_empty()
                || state.token.len() > 128
                || !state
                    .token
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                || state.reason.is_empty()
                || state.reason.len() > 1024
                || state.reason.chars().any(char::is_control)
        })
    {
        return Err(Error::Protocol("Invalid checkout cleanup reply.".into()));
    }
    Ok(reply.cleanup)
}
