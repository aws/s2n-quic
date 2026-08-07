# Security Reporting Policy

## Reporting Security Issues

We kindly ask that you **do not** open a public GitHub issue to report security concerns.

Instead, please submit the issue to the AWS Vulnerability Disclosure Program via [HackerOne](https://hackerone.com/aws_vdp) or send your report via [email](mailto:aws-security@amazon.com).

Amazon Web Services (AWS) practices industry-standard Coordinated Vulnerability Disclosure (CVD) with the goal of reducing adversary advantage while a security vulnerability is being addressed. The [CERT® Guide to Coordinated Vulnerability Disclosure](https://certcc.github.io/CERT-Guide-to-CVD/tutorials/cvd_in_a_nutshell/) provides information about the CVD process, and outlines tools and practices that can help achieve this goal.

For more details, visit the [AWS Vulnerability Reporting Page](http://aws.amazon.com/security/vulnerability-reporting/).

Thank you in advance for collaborating with us to help protect our customers.

## Threat Model

### Shared Responsibility Model

Security is a shared responsibility between s2n-quic and the applications that integrate with it.

s2n-quic is responsible for correctly and securely implementing the QUIC transport protocol ([RFC 9000](https://www.rfc-editor.org/rfc/rfc9000.html)), its use of TLS ([RFC 9001](https://www.rfc-editor.org/rfc/rfc9001.html)), loss detection and congestion control ([RFC 9002](https://www.rfc-editor.org/rfc/rfc9002.html)), and the extensions it supports. It is also responsible for providing safe defaults and secure building blocks for applications to use.

s2n-quic delegates the TLS handshake, certificate validation, and cryptographic algorithm negotiation to a TLS provider, either [s2n-tls](https://github.com/aws/s2n-tls) or [rustls](https://crates.io/crates/rustls). Vulnerabilities in the TLS implementation itself belong to that project's security policy. How s2n-quic drives the provider and applies the resulting keys to QUIC packet protection is in scope here.

Applications integrating with s2n-quic are responsible for the security of the host the process runs on, and for correctly configuring the [providers](https://docs.rs/s2n-quic/latest/s2n_quic/provider/index.html) that govern security-relevant transport behavior, such as connection and endpoint limits, address validation tokens, and connection ID generation.

Given this shared responsibility, the following attacks are considered out of scope for s2n-quic:
* On-host side-channel attacks via CPU/hardware flaws such as Meltdown/Spectre
* Attacks requiring on-host root access to processes, memory, sockets, or files
* Attacks requiring physical access, including fault injection and side channels requiring physical observation
* Traffic analysis based on packet sizes, timing, or counts, which QUIC exposes by design

If you are unsure whether an issue falls in or out of scope, we encourage you to report it; we'd rather investigate a potential concern than miss a real one. Even for out-of-scope attacks, we may still choose to apply mitigations after weighing the potential cost to performance, maintainability, and complexity. All reported findings will be investigated and mitigations will be decided on a case-by-case basis.

### Adversarial Models

These are the threats s2n-quic is designed to defend against. The protection actually achieved depends on how the application configures s2n-quic's providers, on which TLS provider it uses, and on build-time options.

#### Off-Path Adversary

QUIC runs over UDP, so an attacker who cannot observe traffic can still send packets with a spoofed source address to either endpoint. This attacker can:

* Spoof a client's source address to make a server send data to a third party (request forgery and traffic amplification)
* Inject packets into an existing connection in an attempt to have them accepted or to close it
* Send forged Initial packets in volume to exhaust server memory, CPU, or connection state
* Guess a connection ID to elicit a valid stateless reset and tear down a connection it cannot observe (reset oracle)
* Attempt to trigger migration to a path the legitimate peer does not control

#### Network Adversary

An active on-path attacker with complete control over the network between a client and server. In addition to the off-path capabilities above, this attacker can:

* Intercept, modify, reorder, drop, delay, and replay any packet
* Attempt to downgrade the QUIC version negotiated between a client and server
* Exploit timing differences practically measurable over a network
* Modify Initial and Version Negotiation packets, which are not protected against an on-path attacker
* Manipulate loss and congestion signals, for example by remarking ECN codepoints, to degrade or stall a connection
* Record encrypted traffic now for future decryption once a cryptographically relevant quantum computer is available (harvest now, decrypt later)

#### Malicious Client

In addition to the off-path and network adversary capabilities above, a malicious client may:

* Attempt to bypass client certificate-based authentication, where the application has configured mutual authentication
* Send crafted payloads (e.g. transport parameters, coalesced packets) to exploit flaws in parsers
* Attempt to spoof other trusted clients by forging or replaying address validation tokens
* Cause denial of service through resource exhaustion (e.g. excessive streams, floods of small frames or new connection IDs)
* Withhold or forge acknowledgements to manipulate the sender's congestion controller and loss recovery
* Abuse connection migration to direct server traffic at a third party

#### Malicious Server

In addition to the off-path and network adversary capabilities above, a malicious server may:

* Send crafted payloads (e.g. transport parameters, NEW_TOKEN frames) to exploit flaws in parsers
* Advertise transport parameters intended to cause excessive client memory use or unusable packet sizes
* Direct the client to a preferred address in order to have it send packets to a third party (request forgery)

### Vulnerability Scope

Given the adversarial models above, the following are examples of security-relevant issues that should be reported in accordance with [Reporting Security Issues](#reporting-security-issues):

* Implementation defects that compromise confidentiality, integrity, or availability (e.g. undefined behavior in `unsafe` code, panics reachable from peer-controlled input, unbounded memory growth)
* Logic bugs that lead to incorrect QUIC or TLS negotiation, incorrect handshake state transitions, or authentication bypass
* Flaws in packet protection (e.g. nonce or key reuse, incorrect key update, failure to enforce AEAD confidentiality and integrity limits)
* Predictable generation of, or failure to validate, values that must be unguessable, such as connection IDs, address validation tokens, and stateless reset tokens
* Amplification or request forgery beyond what the protocol permits
* Flaws in default configurations or default provider implementations that could lead to insecure operation

The following are generally not considered vulnerabilities in this project's context:

* Application misuse of APIs that behave as documented, including configuring providers in ways the documentation warns against
* Behavior of providers intended only for testing and simulation, such as the packet interceptor and the turmoil IO provider
* Vulnerabilities in the TLS implementation itself, which should be reported to [s2n-tls](https://github.com/aws/s2n-tls/security/policy) or [rustls](https://github.com/rustls/rustls/security/policy)
* Resource exhaustion bounded by limits the application configured and s2n-quic correctly enforced
* Issues in the operating environment (e.g. OS, UDP stack, offload features, hardware)
* Usage patterns documented as warnings in the [API documentation](https://docs.rs/s2n-quic) or the [s2n-quic Guide](https://aws.github.io/s2n-quic/)

## Prenotification Policy

If you package or distribute s2n-quic, or use s2n-quic as part of a large multi-user service, you may be eligible for pre-notification of future s2n-quic releases. Please contact s2n-pre-notification@amazon.com.
