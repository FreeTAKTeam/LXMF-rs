include!("rnode_multi_parts/prelude.rs");

include!("rnode_multi_parts/interface.rs");

include!("rnode_multi_parts/status.rs");

include!("rnode_multi_parts/stream.rs");

include!("rnode_multi_parts/stream_helpers.rs");

#[cfg(test)]
mod tests {
    include!("rnode_multi_parts/tests/core.rs");
}
