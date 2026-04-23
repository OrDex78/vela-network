use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

// ── Wallet file stored locally ────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct WalletFile {
    secret_key_hex: String,
    public_key_hex: String,
    address: String,
}

fn wallet_path() -> PathBuf {
    PathBuf::from("wallet.json")
}

fn load_wallet() -> Result<(SigningKey, String)> {
    let data = fs::read_to_string(wallet_path())?;
    let wf: WalletFile = serde_json::from_str(&data)?;
    let secret_bytes = hex::decode(&wf.secret_key_hex)?;
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&secret_bytes);
    let key = SigningKey::from_bytes(&arr);
    Ok((key, wf.address))
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "vela-cli", about = "Vela Network CLI Wallet")]
struct Cli {
    /// RPC endpoint
    #[arg(long, default_value = "http://localhost:9001")]
    rpc: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a new wallet and save to wallet.json
    NewWallet,
    /// Show your address
    Address,
    /// Check balance of an address (defaults to your wallet)
    Balance {
        #[arg(long)]
        address: Option<String>,
    },
    /// Show node status
    Status,
    /// Get block by height
    Block {
        height: u64,
    },
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::NewWallet => {
            let signing_key = SigningKey::generate(&mut OsRng);
            let verifying_key: VerifyingKey = signing_key.verifying_key();
            let address = format!("vela:{}", hex::encode(verifying_key.to_bytes()));

            let wf = WalletFile {
                secret_key_hex: hex::encode(signing_key.to_bytes()),
                public_key_hex: hex::encode(verifying_key.to_bytes()),
                address: address.clone(),
            };

            fs::write(wallet_path(), serde_json::to_string_pretty(&wf)?)?;

            println!("✅ Wallet created!");
            println!("   Address : {}", address);
            println!("   Saved to: wallet.json");
            println!("   ⚠️  Keep wallet.json safe — it contains your private key!");
        }

        Commands::Address => {
            let (_, address) = load_wallet()?;
            println!("Address: {}", address);
        }

        Commands::Balance { address } => {
            let addr = match address {
                Some(a) => a,
                None => {
                    let (_, a) = load_wallet()?;
                    a
                }
            };
            let url = format!("{}/balance/{}", cli.rpc, addr);
            let resp = reqwest::get(&url).await?.text().await?;
            println!("{}", resp);
        }

        Commands::Status => {
            let url = format!("{}/status", cli.rpc);
            let resp = reqwest::get(&url).await?.text().await?;
            println!("{}", resp);
        }

        Commands::Block { height } => {
            let url = format!("{}/block/{}", cli.rpc, height);
            let resp = reqwest::get(&url).await?.text().await?;
            println!("{}", resp);
        }
    }

    Ok(())
}