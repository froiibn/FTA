# Export-compliance questions — unsent drafts

These drafts contain technical facts, not exemption declarations. The owner must review them and supply any requested legal-entity information. No message has been sent and no government form has been filed.

## Apple export-compliance support

Subject: Clarification of documentation exemption for FTA HTTPS over bundled Rustls/ring

Hello,

We are preparing FTA - File Transfer Anything, App Store app ID 6828538463, version 0.1.2 build 9. The processed build currently shows Missing Compliance. France is included in the planned public, free distribution.

FTA transfers user-selected photos, files, text and URLs directly between recipient-approved devices on a private IPv4 LAN. It has no cloud relay or public messaging service. Both the client and server use HTTP/1 over TLS 1.3: reqwest sends HTTPS requests, and Hyper handles HTTP on a Tokio-Rustls TLS listener. Payloads are HTTP streams or JSON. It uses standard Rustls/ring cryptography, not proprietary algorithms. The app generates self-signed device certificates, performs certificate-pinned mutual TLS, and stores the iOS private key in Keychain. Users can register devices but cannot select algorithms or change encryption strength. There is no separate app-level file-encryption function.

The build's Manage dialog currently starts with the algorithm-type question. We do not see a preceding HTTPS-only or exemption question. Selecting standard algorithms outside the Apple OS and distribution in France in the documentation wizard requests a French encryption declaration copy; we cancelled without saving an exemption or uploading documents.

1. Does any documentation exemption apply to this specific HTTPS-only transport implementation, even though TLS is provided by bundled Rustls/ring and the app also accepts HTTPS connections and generates device certificates?
2. Is the URLSession HTTPS example in your documentation limited to OS-provided cryptography for this purpose? If an exemption applies here, which exact current requirement and questionnaire answer should be used?
3. Is a French declaration required by Apple for these facts, or can an ANSSI confirmation of a specific exemption be supplied instead? What evidence would you require?
4. If an exemption applies, please distinguish Apple upload-document requirements from any separate U.S. classification/reporting and French supply/import obligations.

We have not marked the app as exempt and would appreciate clarification before changing the declaration.

## ANSSI cryptography controls

Subject: Applicability of French declaration exemptions to a consumer LAN file-transfer app

Hello,

We request clarification before publicly distributing a free iOS app, FTA - File Transfer Anything, in France through the App Store. It transfers arbitrary user-selected files, photos, text and URLs between recipient-approved devices on the same private IPv4 LAN. It is not limited to banking, medical use, system administration, DRM or authentication/integrity alone.

Its network traffic is HTTP/1 over TLS 1.3 in both directions using bundled standard Rustls/ring libraries. It generates self-signed device certificates for certificate-pinned mutual TLS; iOS private keys are stored in Keychain. User content is confidential in transit. There is no proprietary encryption algorithm or separate application file-encryption feature. Users install the app normally and can register peers but cannot choose algorithms or encryption strength. No specific French declaration, exemption determination or Grand public validation for this finished app has been obtained.

1. Does supplying this finished app to users in France require a declaration under LCEN Article 30(III) and Décret 2007-663 Article 3, or does a specific current exemption apply? Please identify the provision and conditions.
2. Does ordinary HTTPS/TLS transport, implemented in bundled libraries and also used by an in-app server, qualify for any supply/import exemption not apparent in Annex I?
3. We understand Annex I E12 concerns personal-use/development imports or transfers and does not itself exempt a developer's public supply. Is that interpretation correct for App Store distribution?
4. We understand Annex II B3/Grand public affects export authorization and requires ANSSI validation, rather than automatically waiving the French supply/import declaration. Is that correct for this app?
5. If a library already has a declaration, can it cover a finished application incorporating it under any conditions, or must this application be declared separately? We have not established that a relevant declaration exists for its exact bundled libraries/version.
6. If a declaration is required, which current form, supporting technical documents, and acknowledgement should be used, and what document can be provided to Apple? Do requirements differ for an individual, a company, or a supplier outside France? Please distinguish a supply declaration from an export authorization.

We can provide the precise technical configuration for review. Please advise what legal-entity information is needed; none is asserted in this preliminary technical question.
