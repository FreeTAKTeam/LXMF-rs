## Reticulum belongs to everyone: Our position on independent implementations

Recent statements by Reticulum's original author, Mark Qvist, raise serious concerns about the future of RNS independent implementations like this one, especially using AI-assisted development.

His position contains several fundamental contradictions:

1. **The Reticulum protocol is public domain.** Qvist has repeatedly declared that anyone may implement it, including under independent commercial or open-source licences.
2. **The Python reference implementation is the authoritative specification.** He explicitly states that no separate formal specification or RFC will ever be published.
3. **AI-assisted reimplementations are now publicly challenged**, with allegations that some constitute unauthorized derivative works and that their software licences are invalid.

This raises an unavoidable question:

**How can developers independently implement a protocol when its only complete authoritative description is the original implementation, yet using modern tools to examine and understand that implementation is treated with suspicion?**

And why should using an AI assistant to understand software be fundamentally different from asking a human engineer to perform the same analysis?

### Copyright protects expression, not functionality

The legal distinction is crucial.

Understanding an implementation, reproducing protocol behaviour, and copying protected source-code expression are different activities.

A mechanical Python-to-Rust translation may constitute a derivative work. 
However, independently implementing the same wire formats, cryptographic operations, state transitions and interoperability requirements does not automatically constitute copyright infringement.
The use of AI does not, by itself, determine whether protected expression has been copied.

Furthermore, the [Reticulum License](https://github.com/markqvist/Reticulum/blob/master/LICENSE) explicitly restricts the use of its software for AI training. It does not expressly establish a blanket prohibition on AI-assisted source-code analysis. The actual licence conditions and the circumstances of each implementation must be examined.

**Legal conclusions require evidence, not declarations by a project's original author.**

### Where is the conformance test suite?

There is also a significant engineering problem.

Without an independent, normative technical specification and a publicly maintained conformance test suite, alternative implementations must reconstruct protocol requirements from documentation, observed behaviour and the Python reference.

This makes compatibility harder to establish and disagreements more difficult to resolve.

The appropriate engineering response is to publish reproducible test vectors, interoperability requirements and an objective **Protocol Conformance Test Suite** against which every implementation (including the original Python version!) can be evaluated.

Compatibility should be demonstrated through evidence, not determined by the "personal approval" of any individual.

### Our commitment

* LXMF-rs will continue to be developed as a Rust-native implementation of Reticulum and LXMF.*

We respect Mark Qvist's original contribution and his legitimate copyright interests. We will continue to respect applicable licences, document implementation decisions, and maintain independently reproducible interoperability and compatibility tests.

However, **we reject the proposition that AI-assisted reimplementations are inherently unlawful, or that the creator of a public-domain protocol can exercise exclusive control over its independent implementation.**

The purpose of Reticulum is decentralized, resilient communication. Its future should be secured through interoperability, technical excellence and the freedom to innovate.

**Reticulum is too important to depend on, or be controlled by, any single individual, including its original creator.**

---

**References:** [Reticulum's authoritative specification statement](https://github.com/markqvist/Reticulum#reference-implementation) · [Qvist's invitation to continue development](https://github.com/markqvist/Reticulum/blob/master/MIRROR.md) · [Reticulum License](https://github.com/markqvist/Reticulum/blob/master/LICENSE) · [17 USC §102(b)](https://www.law.cornell.edu/uscode/text/17/102) · [SAS Institute v. World Programming, C-406/10](https://eur-lex.europa.eu/legal-content/EN/TXT/?uri=CELEX:62010CJ0406)
