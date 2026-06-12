use base64::Engine as _;

use clap::{Parser, ValueEnum};

use rns_rpc::e2e_harness::timestamp_millis;

use sha2::{Digest, Sha256};

use std::fs;

use std::io::{self, Write};

use std::path::PathBuf;
