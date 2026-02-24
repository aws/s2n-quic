// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bytes::{Bytes, BytesMut};
use core::{ffi::c_void, marker::PhantomData};
use s2n_quic_core::{
    application::ServerName,
    crypto::{
        tls,
        tls::{CipherSuite, NamedGroup},
        CryptoSuite,
    },
    endpoint, transport,
};
use s2n_quic_crypto::{
    aws_lc_aead as aead, handshake::HandshakeKey, hkdf, one_rtt::OneRttKey, Prk, SecretPair, Suite,
};
use s2n_tls::{connection::Connection, error::Fallible, ffi::*};

/// The preallocated size of the outgoing buffer
///
/// By allocating a larger buffer, we only do a few allocations, even when
/// s2n-tls sends small chunks
const SEND_BUFFER_CAPACITY: usize = 2048;

/// Handles all callback contexts for each session
pub struct Callback<'a, T, C> {
    pub context: &'a mut T,
    pub endpoint: endpoint::Type,
    pub state: &'a mut State,
    pub suite: PhantomData<C>,
    pub err: Option<transport::Error>,
    pub send_buffer: &'a mut BytesMut,
    pub server_name: &'a mut Option<ServerName>,
    pub server_params: &'a mut Vec<u8>,
}

impl<'a, T, C> Callback<'a, T, C>
where
    T: 'a + tls::Context<C>,
    C: CryptoSuite<
        HandshakeKey = <Suite as CryptoSuite>::HandshakeKey,
        HandshakeHeaderKey = <Suite as CryptoSuite>::HandshakeHeaderKey,
        InitialKey = <Suite as CryptoSuite>::InitialKey,
        InitialHeaderKey = <Suite as CryptoSuite>::InitialHeaderKey,
        OneRttKey = <Suite as CryptoSuite>::OneRttKey,
        OneRttHeaderKey = <Suite as CryptoSuite>::OneRttHeaderKey,
        ZeroRttKey = <Suite as CryptoSuite>::ZeroRttKey,
        ZeroRttHeaderKey = <Suite as CryptoSuite>::ZeroRttHeaderKey,
        RetryKey = <Suite as CryptoSuite>::RetryKey,
    >,
{
    /// Initializes the s2n-tls connection with all of the contexts and callbacks
    ///
    /// # Safety
    ///
    /// * The Callback struct must live at least as long as the connection
    /// * or the `unset` method should be called if it doesn't
    pub unsafe fn set(&mut self, connection: &mut Connection) {
        let context = self as *mut Self as *mut c_void;

        // We use unwrap here since s2n-tls will just check if connection is not null
        connection
            .set_secret_callback(Some(Self::secret_cb), context)
            .unwrap();
        connection.set_send_callback(Some(Self::send_cb)).unwrap();
        connection.set_send_context(context).unwrap();
        connection
            .set_receive_callback(Some(Self::recv_cb))
            .unwrap();
        connection.set_receive_context(context).unwrap();
        // A Waker is provided for use with the client hello callback.
        connection.set_waker(Some(self.context.waker())).unwrap();
    }

    /// Removes all of the callback and context pointers from the connection
    pub fn unset(mut self, connection: &mut Connection) -> Result<(), transport::Error> {
        unsafe {
            unsafe extern "C" fn secret_cb(
                _context: *mut c_void,
                _conn: *mut s2n_connection,
                _secret_type: s2n_secret_type_t::Type,
                _secret: *mut u8,
                _secret_size: u8,
            ) -> s2n_status_code::Type {
                -1
            }

            unsafe extern "C" fn send_cb(
                _context: *mut c_void,
                _data: *const u8,
                _len: u32,
            ) -> s2n_status_code::Type {
                -1
            }

            unsafe extern "C" fn recv_cb(
                _context: *mut c_void,
                _data: *mut u8,
                _len: u32,
            ) -> s2n_status_code::Type {
                -1
            }

            // We use unwrap here since s2n-tls will just check if connection is not null
            connection
                .set_secret_callback(Some(secret_cb), core::ptr::null_mut())
                .unwrap();
            connection.set_send_callback(Some(send_cb)).unwrap();
            connection.set_send_context(core::ptr::null_mut()).unwrap();
            connection.set_receive_callback(Some(recv_cb)).unwrap();
            connection
                .set_receive_context(core::ptr::null_mut())
                .unwrap();
            connection.set_waker(None).unwrap();

            // Flush the send buffer before returning to the connection
            self.flush();

            if let Some(server_name) = self.server_name.take() {
                self.context.on_server_name(server_name)?;
            }

            if let Some(err) = self.err {
                return Err(err);
            }

            Ok(())
        }
    }

    /// The function s2n-tls calls when it emits secrets
    unsafe extern "C" fn secret_cb(
        context: *mut c_void,
        conn: *mut s2n_connection,
        secret_type: s2n_secret_type_t::Type,
        secret: *mut u8,
        secret_size: u8,
    ) -> s2n_status_code::Type {
        let context = &mut *(context as *mut Self);
        let secret = core::slice::from_raw_parts_mut(secret, secret_size as _);
        match context.on_secret(conn, secret_type, secret) {
            Ok(()) => 0,
            Err(err) => {
                context.err = Some(err);
                -1
            }
        }
    }

    /// Handles secrets from the s2n-tls connection
    fn on_secret(
        &mut self,
        conn: *mut s2n_connection,
        id: s2n_secret_type_t::Type,
        secret: &mut [u8],
    ) -> Result<(), transport::Error> {
        match core::mem::replace(&mut self.state.secrets, Secrets::Waiting) {
            Secrets::Waiting => {
                if id == s2n_secret_type_t::CLIENT_EARLY_TRAFFIC_SECRET {
                    return Ok(());
                }

                let (prk_algo, _aead, cipher_suite) =
                    get_algo_type(conn).ok_or(tls::Error::INTERNAL_ERROR)?;
                let secret = Prk::new_less_safe(prk_algo, secret);
                self.state.secrets = Secrets::Half { secret, id };
                self.state.cipher_suite = cipher_suite;

                Ok(())
            }
            Secrets::Half {
                id: other_id,
                secret: other_secret,
            } => {
                let (prk_algo, aead_algo, cipher_suite) =
                    get_algo_type(conn).ok_or(tls::Error::INTERNAL_ERROR)?;
                let secret = Prk::new_less_safe(prk_algo, secret);
                self.state.cipher_suite = cipher_suite;
                let pair = match (id, other_id) {
                    (
                        s2n_secret_type_t::CLIENT_HANDSHAKE_TRAFFIC_SECRET,
                        s2n_secret_type_t::SERVER_HANDSHAKE_TRAFFIC_SECRET,
                    )
                    | (
                        s2n_secret_type_t::CLIENT_APPLICATION_TRAFFIC_SECRET,
                        s2n_secret_type_t::SERVER_APPLICATION_TRAFFIC_SECRET,
                    ) => SecretPair {
                        client: secret,
                        server: other_secret,
                    },
                    (
                        s2n_secret_type_t::SERVER_HANDSHAKE_TRAFFIC_SECRET,
                        s2n_secret_type_t::CLIENT_HANDSHAKE_TRAFFIC_SECRET,
                    )
                    | (
                        s2n_secret_type_t::SERVER_APPLICATION_TRAFFIC_SECRET,
                        s2n_secret_type_t::CLIENT_APPLICATION_TRAFFIC_SECRET,
                    ) => SecretPair {
                        server: secret,
                        client: other_secret,
                    },
                    _ => {
                        debug_assert!(false, "invalid key phase");
                        return Err(transport::Error::INTERNAL_ERROR);
                    }
                };

                // Flush the send buffer before transitioning to the next phase
                self.flush();

                match self.state.tx_phase {
                    HandshakePhase::Initial => {
                        let (key, header_key) = HandshakeKey::new(self.endpoint, aead_algo, pair)
                            .expect("invalid cipher");

                        if !self.server_params.is_empty() {
                            debug_assert!(self.endpoint.is_server());

                            // Since the client transport parameters are sent in the Initial packet space
                            // we can access them early on in the handshake, allowing the server transport
                            // parameters to be configured based on the client's parameters.
                            unsafe {
                                // Safety: conn needs to outlive params
                                let client_params = get_application_params(conn)?;

                                // Allow for additional server params to be appended that depend
                                // on the given client params
                                self.context.on_client_application_params(
                                    client_params,
                                    self.server_params,
                                )?;

                                // Now set the server transport parameters on the connection for
                                // transmission to the client
                                s2n_connection_set_quic_transport_parameters(
                                    conn,
                                    self.server_params.as_ptr(),
                                    self.server_params.len() as _,
                                );
                            }
                            // We've given the transport parameters to s2n-tls, so clear
                            // `server_params` to ensure they are not set again.
                            self.server_params.clear();
                        }

                        self.context.on_handshake_keys(key, header_key)?;
                        self.state.tx_phase.transition();
                        self.state.rx_phase.transition();
                    }
                    _ => {
                        let (key, header_key) =
                            OneRttKey::new(self.endpoint, aead_algo, pair).expect("invalid cipher");
                        // At this point the server is done writing Handshake messages
                        if self.endpoint.is_server() {
                            self.state.tx_phase.transition();
                        }
                        let params = unsafe {
                            // Safety: conn needs to outlive params
                            let application_protocol =
                                Bytes::copy_from_slice(get_application_protocol(conn)?);
                            self.context.on_application_protocol(application_protocol)?;

                            // Client has already emitted the server name elsewhere
                            if self.endpoint.is_server() {
                                if let Some(server_name) = get_server_name(conn) {
                                    self.context.on_server_name(server_name)?;
                                }
                            }

                            if let Some(named_group) = get_key_exchange_group(conn) {
                                self.context.on_key_exchange_group(named_group)?;
                            }

                            get_application_params(conn)?
                        };

                        self.context.on_one_rtt_keys(key, header_key, params)?;
                    }
                }

                Ok(())
            }
        }
    }

    /// The function s2n-tls calls when it wants to send data
    unsafe extern "C" fn send_cb(
        context: *mut c_void,
        data: *const u8,
        len: u32,
    ) -> s2n_status_code::Type {
        let context = &mut *(context as *mut Self);
        let data = core::slice::from_raw_parts(data, len as _);
        context.on_write(data) as _
    }

    /// Called when sending data
    fn on_write(&mut self, data: &[u8]) -> usize {
        // If this write would cause the current send buffer to reallocate,
        // we should flush and create a new send buffer.
        let remaining_capacity = self.send_buffer.capacity() - self.send_buffer.len();

        if remaining_capacity < data.len() {
            // Flush the send buffer before reallocating it
            self.flush();

            // ensure we only do one allocation for this write
            let len = SEND_BUFFER_CAPACITY.max(data.len());

            debug_assert!(
                self.send_buffer.is_empty(),
                "dropping a send buffer with data will result in data loss"
            );
            *self.send_buffer = BytesMut::with_capacity(len);
        }

        // Write the current data to the send buffer
        //
        // NOTE: we don't immediately flush to the packet space since s2n-tls may do
        //       several small writes in a row.
        self.send_buffer.extend_from_slice(data);

        data.len()
    }

    /// Flushes the send buffer into the current TX space
    fn flush(&mut self) {
        if !self.send_buffer.is_empty() {
            let chunk = self.send_buffer.split().freeze();

            match self.state.tx_phase {
                HandshakePhase::Initial => self.context.send_initial(chunk),
                HandshakePhase::Handshake => self.context.send_handshake(chunk),
                HandshakePhase::Application => self.context.send_application(chunk),
            }
        }
    }

    /// The function s2n-tls calls when it wants to receive data
    unsafe extern "C" fn recv_cb(
        context: *mut c_void,
        data: *mut u8,
        len: u32,
    ) -> s2n_status_code::Type {
        let context = &mut *(context as *mut Self);
        let data = core::slice::from_raw_parts_mut(data, len as _);
        match context.on_read(data) {
            0 => {
                // https://github.com/aws/s2n-tls/blob/main/docs/USAGE-GUIDE.md#s2n_connection_set_send_cb
                // s2n-tls wants us to set the global errno to signal blocked
                errno::set_errno(errno::Errno(libc::EWOULDBLOCK));
                -1
            }
            len => len as _,
        }
    }

    /// Called when receiving data
    fn on_read(&mut self, data: &mut [u8]) -> usize {
        let max_len = Some(data.len());

        // This is a semi-random number. However, it happens to work out OK to approximate
        // spend during handshakes. On one side of a connection a handshake typically costs about
        // 0.5-1ms of CPU. Most of that is driven by the peer sending information that the local
        // endpoint acts on (e.g., verifying signatures).
        //
        // In practice this was chosen to make s2n-quic-sim simulate an uncontended mTLS handshake
        // as taking 2ms (in combination with the transition edge adding some extra cost), which is
        // fairly close to what we see in one scenario with real handshakes.
        s2n_quic_core::io::event_loop::attribute_cpu(core::time::Duration::from_micros(100));

        let chunk = match self.state.rx_phase {
            HandshakePhase::Initial => self.context.receive_initial(max_len),
            HandshakePhase::Handshake => self.context.receive_handshake(max_len),
            HandshakePhase::Application => self.context.receive_application(max_len),
        };

        if let Some(chunk) = chunk {
            let len = chunk.len();
            data[..len].copy_from_slice(&chunk);
            len
        } else {
            0
        }
    }
}

#[derive(Default, Debug)]
pub struct State {
    rx_phase: HandshakePhase,
    tx_phase: HandshakePhase,
    secrets: Secrets,
    cipher_suite: CipherSuite,
}

impl State {
    /// Complete the handshake
    pub fn on_handshake_complete(&mut self) {
        self.tx_phase.transition();
        self.rx_phase.transition();
        debug_assert_eq!(self.tx_phase, HandshakePhase::Application);
        debug_assert_eq!(self.rx_phase, HandshakePhase::Application);
    }

    pub fn cipher_suite(&self) -> CipherSuite {
        self.cipher_suite
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Default)]
enum HandshakePhase {
    #[default]
    Initial,
    Handshake,
    Application,
}

impl HandshakePhase {
    fn transition(&mut self) {
        // See comment in `on_read` for value and why this exists.
        s2n_quic_core::io::event_loop::attribute_cpu(core::time::Duration::from_micros(100));

        *self = match self {
            Self::Initial => Self::Handshake,
            _ => Self::Application,
        };
    }
}

#[derive(Debug, Default)]
enum Secrets {
    #[default]
    Waiting,
    Half {
        secret: Prk,
        id: s2n_secret_type_t::Type,
    },
}

fn get_algo_type(
    connection: *mut s2n_connection,
) -> Option<(hkdf::Algorithm, &'static aead::Algorithm, CipherSuite)> {
    let mut cipher = [0, 0];
    unsafe {
        s2n_connection_get_cipher_iana_value(connection, &mut cipher[0], &mut cipher[1])
            .into_result()
            .ok()?;
    }

    //= https://www.rfc-editor.org/rfc/rfc8446#appendix-B.4
    //# This specification defines the following cipher suites for use with
    //# TLS 1.3.
    //#
    //#              +------------------------------+-------------+
    //#              | Description                  | Value       |
    //#              +------------------------------+-------------+
    //#              | TLS_AES_128_GCM_SHA256       | {0x13,0x01} |
    //#              |                              |             |
    //#              | TLS_AES_256_GCM_SHA384       | {0x13,0x02} |
    //#              |                              |             |
    //#              | TLS_CHACHA20_POLY1305_SHA256 | {0x13,0x03} |
    //#              |                              |             |
    //#              | TLS_AES_128_CCM_SHA256       | {0x13,0x04} |
    //#              |                              |             |
    //#              | TLS_AES_128_CCM_8_SHA256     | {0x13,0x05} |
    //#              +------------------------------+-------------+
    const TLS_AES_128_GCM_SHA256: [u8; 2] = [0x13, 0x01];
    const TLS_AES_256_GCM_SHA384: [u8; 2] = [0x13, 0x02];
    const TLS_CHACHA20_POLY1305_SHA256: [u8; 2] = [0x13, 0x03];

    // NOTE: we don't have CCM support implemented currently

    // NOTE: CCM_8 is not allowed by QUIC
    //= https://www.rfc-editor.org/rfc/rfc9001#section-5.3
    //# QUIC can use any of the cipher suites defined in [TLS13] with the
    //# exception of TLS_AES_128_CCM_8_SHA256.

    match cipher {
        TLS_AES_128_GCM_SHA256 => Some((
            hkdf::HKDF_SHA256,
            &aead::AES_128_GCM,
            CipherSuite::TLS_AES_128_GCM_SHA256,
        )),
        TLS_AES_256_GCM_SHA384 => Some((
            hkdf::HKDF_SHA384,
            &aead::AES_256_GCM,
            CipherSuite::TLS_AES_256_GCM_SHA384,
        )),
        TLS_CHACHA20_POLY1305_SHA256 => Some((
            hkdf::HKDF_SHA256,
            &aead::CHACHA20_POLY1305,
            CipherSuite::TLS_CHACHA20_POLY1305_SHA256,
        )),
        _ => None,
    }
}

unsafe fn get_application_params<'a>(
    connection: *mut s2n_connection,
) -> Result<tls::ApplicationParameters<'a>, tls::Error> {
    //= https://www.rfc-editor.org/rfc/rfc9001#section-8.1
    //# When using ALPN, endpoints MUST immediately close a connection (see
    //# Section 10.2 of [QUIC-TRANSPORT]) with a no_application_protocol TLS
    //# alert (QUIC error code 0x178; see Section 4.8) if an application
    //# protocol is not negotiated.

    //= https://www.rfc-editor.org/rfc/rfc9001#section-8.1
    //# While [ALPN] only specifies that servers
    //# use this alert, QUIC clients MUST use error 0x178 to terminate a
    //# connection when ALPN negotiation fails.
    let transport_parameters =
        get_transport_parameters(connection).ok_or(tls::Error::MISSING_EXTENSION)?;

    Ok(tls::ApplicationParameters {
        transport_parameters,
    })
}

//= https://www.rfc-editor.org/rfc/rfc9001#section-8.1
//# Unless
//# another mechanism is used for agreeing on an application protocol,
//# endpoints MUST use ALPN for this purpose.
//
//= https://www.rfc-editor.org/rfc/rfc7301#section-3.1
//# Client                                              Server
//#
//#    ClientHello                     -------->       ServerHello
//#      (ALPN extension &                               (ALPN extension &
//#       list of protocols)                              selected protocol)
//#                                                    [ChangeCipherSpec]
//#                                    <--------       Finished
//#    [ChangeCipherSpec]
//#    Finished                        -------->
//#    Application Data                <------->       Application Data
unsafe fn get_application_protocol<'a>(
    connection: *mut s2n_connection,
) -> Result<&'a [u8], tls::Error> {
    let ptr = s2n_get_application_protocol(connection).into_result().ok();
    ptr.and_then(|ptr| get_cstr_slice(ptr))
        .ok_or(tls::Error::MISSING_EXTENSION)
}

unsafe fn get_server_name(connection: *mut s2n_connection) -> Option<ServerName> {
    let ptr = s2n_get_server_name(connection).into_result().ok()?;
    let data = get_cstr_slice(ptr)?;

    // validate sni is a valid UTF-8 string
    let string = core::str::from_utf8(data).ok()?;
    Some(string.into())
}

unsafe fn get_key_exchange_group(connection: *mut s2n_connection) -> Option<NamedGroup> {
    let mut group_name = core::ptr::null();
    s2n_connection_get_key_exchange_group(connection, &mut group_name)
        .into_result()
        .ok()?;
    let group_name = get_cstr_slice(group_name)?;
    let group_name = core::str::from_utf8(group_name).ok()?;

    let contains_kem = get_kem_group_name(connection).is_some();

    Some(NamedGroup {
        group_name,
        contains_kem,
    })
}

unsafe fn get_kem_group_name(connection: *mut s2n_connection) -> Option<&'static str> {
    let ptr = s2n_connection_get_kem_group_name(connection)
        .into_result()
        .ok()?;
    let data = get_cstr_slice(ptr)?;
    let string = core::str::from_utf8(data).ok()?;

    if string == "NONE" {
        return None;
    }
    Some(string)
}

unsafe fn get_transport_parameters<'a>(connection: *mut s2n_connection) -> Option<&'a [u8]> {
    let mut ptr = core::ptr::null();
    let mut len = 0u16;

    s2n_connection_get_quic_transport_parameters(connection, &mut ptr, &mut len)
        .into_result()
        .ok()?;

    get_slice(ptr, len as _)
}

unsafe fn get_cstr_slice<'a>(ptr: *const libc::c_char) -> Option<&'a [u8]> {
    let len = libc::strlen(ptr);
    get_slice(ptr as *const _, len)
}

unsafe fn get_slice<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if ptr.is_null() || len == 0 {
        return None;
    }

    Some(core::slice::from_raw_parts(ptr, len as _))
}
