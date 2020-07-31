





Network Working Group                                        M. Blanchet
Request for Comments: 5156                                      Viagenie
Category: Informational                                       April 2008


                       Special-Use IPv6 Addresses

Status of This Memo

   This memo provides information for the Internet community.  It does
   not specify an Internet standard of any kind.  Distribution of this
   memo is unlimited.

Abstract

   This document is a compilation of special IPv6 addresses defined in
   other RFCs.  It can be used as a checklist of invalid routing
   prefixes for developing filtering policies for routes and IP packets.
   It does not discuss addresses that are assigned to operators and
   users through the Regional Internet Registries.

Table of Contents

   1.  Introduction  . . . . . . . . . . . . . . . . . . . . . . . . . 2
   2.  Address Blocks  . . . . . . . . . . . . . . . . . . . . . . . . 2
     2.1.  Node-Scoped Unicast . . . . . . . . . . . . . . . . . . . . 2
     2.2.  IPv4-Mapped Addresses . . . . . . . . . . . . . . . . . . . 2
     2.3.  IPv4-Compatible Addresses . . . . . . . . . . . . . . . . . 2
     2.4.  Link-Scoped Unicast . . . . . . . . . . . . . . . . . . . . 2
     2.5.  Unique-Local  . . . . . . . . . . . . . . . . . . . . . . . 3
     2.6.  Documentation Prefix  . . . . . . . . . . . . . . . . . . . 3
     2.7.  6to4  . . . . . . . . . . . . . . . . . . . . . . . . . . . 3
     2.8.  Teredo  . . . . . . . . . . . . . . . . . . . . . . . . . . 3
     2.9.  6bone . . . . . . . . . . . . . . . . . . . . . . . . . . . 3
     2.10. ORCHID  . . . . . . . . . . . . . . . . . . . . . . . . . . 3
     2.11. Default Route . . . . . . . . . . . . . . . . . . . . . . . 4
     2.12. IANA Special-Purpose IPv6 Address Registry  . . . . . . . . 4
     2.13. Multicast . . . . . . . . . . . . . . . . . . . . . . . . . 4
   3.  Security Considerations . . . . . . . . . . . . . . . . . . . . 4
   4.  IANA Considerations . . . . . . . . . . . . . . . . . . . . . . 4
   5.  Acknowledgements  . . . . . . . . . . . . . . . . . . . . . . . 4
   6.  References  . . . . . . . . . . . . . . . . . . . . . . . . . . 5
     6.1.  Normative References  . . . . . . . . . . . . . . . . . . . 5
     6.2.  Informative References  . . . . . . . . . . . . . . . . . . 5







Blanchet                     Informational                      [Page 1]

RFC 5156               Special-Use IPv6 Addresses             April 2008


1.  Introduction

   This document is a compilation of special IPv6 addresses defined in
   other RFCs.  It can be used as a checklist of invalid routing
   prefixes for developing filtering policies for routes and IP packets.
   It does not discuss addresses that are assigned to operators and
   users through the Regional Internet Registries.

   The document is structured by address types.  The document format is
   similar to [RFC3330].

   Some tips about filtering are given, but are not mandatory to
   implement.

   The addresses listed in this document must not be hard-coded into
   implementations.

2.  Address Blocks

2.1.  Node-Scoped Unicast

   ::1/128 is the loopback address [RFC4291].

   ::/128 is the unspecified address [RFC4291].

   These two addresses should not appear on the public Internet.

2.2.  IPv4-Mapped Addresses

   ::FFFF:0:0/96 are the IPv4-mapped addresses [RFC4291].  Addresses
   within this block should not appear on the public Internet.

2.3.  IPv4-Compatible Addresses

   ::<ipv4-address>/96 are the IPv4-compatible addresses [RFC4291].
   These addresses are deprecated and should not appear on the public
   Internet.

2.4.  Link-Scoped Unicast

   fe80::/10 are the link-local unicast [RFC4291] addresses.  Addresses
   within this block should not appear on the public Internet.









Blanchet                     Informational                      [Page 2]

RFC 5156               Special-Use IPv6 Addresses             April 2008


2.5.  Unique-Local

   fc00::/7 are the unique-local addresses [RFC4193].  Addresses within
   this block should not appear by default on the public Internet.
   Procedures for advertising these addresses are further described in
   [RFC4193].

2.6.  Documentation Prefix

   The 2001:db8::/32 are the documentation addresses [RFC3849].  They
   are used for documentation purposes such as user manuals, RFCs, etc.
   Addresses within this block should not appear on the public Internet.

2.7.  6to4

   2002::/16 are the 6to4 addresses [RFC3056].  The 6to4 addresses may
   be advertised when the site is running a 6to4 relay or offering a
   6to4 transit service.  Running such a service [RFC3964] entails
   filtering rules specific to 6to4 [RFC3964].  IPv4 addresses
   disallowed in 6to4 prefixes are listed in section 5.3.1 of [RFC3964].

2.8.  Teredo

   2001::/32 are the Teredo addresses [RFC4380].  The Teredo addresses
   may be advertised when the site is running a Teredo relay or offering
   a Teredo transit service.

2.9.  6bone

   5f00::/8 were the addresses of the first instance of the 6bone
   experimental network [RFC1897].

   3ffe::/16 were the addresses of the second instance of the 6bone
   experimental network [RFC2471].

   Both 5f00::/8 and 3ffe::/16 were returned to IANA [RFC3701].  These
   addresses are subject to future allocation, similar to current
   unallocated address space.  Addresses within these blocks should not
   appear on the public Internet until they are reallocated.

2.10.  ORCHID

   2001:10::/28 are Overlay Routable Cryptographic Hash IDentifiers
   (ORCHID) addresses [RFC4843].  These addresses are used as
   identifiers and are not routable at the IP layer.  Addresses within
   this block should not appear on the public Internet.





Blanchet                     Informational                      [Page 3]

RFC 5156               Special-Use IPv6 Addresses             April 2008


2.11.  Default Route

   ::/0 is the default unicast route address.

2.12.  IANA Special-Purpose IPv6 Address Registry

   An IANA registry (iana-ipv6-special-registry) exists [RFC4773] for
   Special-Purpose IPv6 address block assignments for experiments and
   other purposes.  Addresses within this registry should be reviewed
   for Internet routing considerations.

2.13.  Multicast

   ff00::/8 are multicast addresses [RFC4291].  They contain a 4-bit
   scope in the address field where only some values are of global scope
   [RFC4291].  Only addresses with global scope in this block may appear
   on the public Internet.

   Multicast routes must not appear in unicast routing tables.

3.  Security Considerations

   Filtering the invalid routing prefixes listed in this document should
   improve the security of networks.

4.  IANA Considerations

   To ensure consistency and to provide cross-referencing for the
   benefit of the community, IANA has inserted the following paragraph
   in the header of the iana-ipv6-special-registry.

   "Other special IPv6 addresses requiring specific considerations for
   global routing are listed in RFC 5156."

5.  Acknowledgements

   Florent Parent, Pekka Savola, Tim Chown, Alain Baudot, Stig Venaas,
   Vincent Jardin, Olaf Bonness, David Green, Gunter Van de Velde,
   Michael Barnes, Fred Baker, Edward Lewis, Marla Azinger, Brian
   Carpenter, Mark Smith, Kevin Loch, Alain Durand, Jim Bound, Peter
   Sherbin, Bob Hinden, Gert Doering, Niall O'Reilly, Mark Townsley,
   Jari Arkko, and Iain Calder have provided input and suggestions to
   this document.








Blanchet                     Informational                      [Page 4]

RFC 5156               Special-Use IPv6 Addresses             April 2008


6.  References

6.1.  Normative References

   [RFC4291]  Hinden, R. and S. Deering, "IP Version 6 Addressing
              Architecture", RFC 4291, February 2006.

6.2.  Informative References

   [RFC1897]  Hinden, R. and J. Postel, "IPv6 Testing Address
              Allocation", RFC 1897, January 1996.

   [RFC2471]  Hinden, R., Fink, R., and J. Postel, "IPv6 Testing Address
              Allocation", RFC 2471, December 1998.

   [RFC3056]  Carpenter, B. and K. Moore, "Connection of IPv6 Domains
              via IPv4 Clouds", RFC 3056, February 2001.

   [RFC3330]  IANA, "Special-Use IPv4 Addresses", RFC 3330,
              September 2002.

   [RFC3701]  Fink, R. and R. Hinden, "6bone (IPv6 Testing Address
              Allocation) Phaseout", RFC 3701, March 2004.

   [RFC3849]  Huston, G., Lord, A., and P. Smith, "IPv6 Address Prefix
              Reserved for Documentation", RFC 3849, July 2004.

   [RFC3964]  Savola, P. and C. Patel, "Security Considerations for
              6to4", RFC 3964, December 2004.

   [RFC4193]  Hinden, R. and B. Haberman, "Unique Local IPv6 Unicast
              Addresses", RFC 4193, October 2005.

   [RFC4380]  Huitema, C., "Teredo: Tunneling IPv6 over UDP through
              Network Address Translations (NATs)", RFC 4380,
              February 2006.

   [RFC4773]  Huston, G., "Administration of the IANA Special Purpose
              IPv6 Address Block", RFC 4773, December 2006.

   [RFC4843]  Nikander, P., Laganier, J., and F. Dupont, "An IPv6 Prefix
              for Overlay Routable Cryptographic Hash Identifiers
              (ORCHID)", RFC 4843, April 2007.








Blanchet                     Informational                      [Page 5]

RFC 5156               Special-Use IPv6 Addresses             April 2008


Author's Address

   Marc Blanchet
   Viagenie
   2600 boul. Laurier, suite 625
   Quebec, QC  G1V 4W1
   Canada

   EMail: Marc.Blanchet@viagenie.ca
   URI:   http://www.viagenie.ca









































Blanchet                     Informational                      [Page 6]

RFC 5156               Special-Use IPv6 Addresses             April 2008


Full Copyright Statement

   Copyright (C) The IETF Trust (2008).

   This document is subject to the rights, licenses and restrictions
   contained in BCP 78, and except as set forth therein, the authors
   retain all their rights.

   This document and the information contained herein are provided on an
   "AS IS" basis and THE CONTRIBUTOR, THE ORGANIZATION HE/SHE REPRESENTS
   OR IS SPONSORED BY (IF ANY), THE INTERNET SOCIETY, THE IETF TRUST AND
   THE INTERNET ENGINEERING TASK FORCE DISCLAIM ALL WARRANTIES, EXPRESS
   OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTY THAT THE USE OF
   THE INFORMATION HEREIN WILL NOT INFRINGE ANY RIGHTS OR ANY IMPLIED
   WARRANTIES OF MERCHANTABILITY OR FITNESS FOR A PARTICULAR PURPOSE.

Intellectual Property

   The IETF takes no position regarding the validity or scope of any
   Intellectual Property Rights or other rights that might be claimed to
   pertain to the implementation or use of the technology described in
   this document or the extent to which any license under such rights
   might or might not be available; nor does it represent that it has
   made any independent effort to identify any such rights.  Information
   on the procedures with respect to rights in RFC documents can be
   found in BCP 78 and BCP 79.

   Copies of IPR disclosures made to the IETF Secretariat and any
   assurances of licenses to be made available, or the result of an
   attempt made to obtain a general license or permission for the use of
   such proprietary rights by implementers or users of this
   specification can be obtained from the IETF on-line IPR repository at
   http://www.ietf.org/ipr.

   The IETF invites any interested party to bring to its attention any
   copyrights, patents or patent applications, or other proprietary
   rights that may cover technology that may be required to implement
   this standard.  Please address the information to the IETF at
   ietf-ipr@ietf.org.












Blanchet                     Informational                      [Page 7]

