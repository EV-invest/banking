//! Diffs two copies of one TigerBeetle ledger: what a cluster committed after a
//! timestamp, and whether another cluster holds exactly that.
//!
//! ```text
//! ledger-gap --cluster-id <u128> watermark --addresses <a,b,c> [--before <ts>]
//! ledger-gap --cluster-id <u128> export    --addresses <a,b,c> --since <ts> --out <file>
//! ledger-gap --cluster-id <u128> check     --addresses <a,b,c> --gap <file>
//! ```

use std::{
	collections::{BTreeMap, HashMap},
	future::Future,
	process::ExitCode,
	time::Duration,
};

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use piggybank_core::infrastructure::tigerbeetle::TigerBeetle;
use serde::{Deserialize, Serialize};
use tigerbeetle as tb;

/// TigerBeetle's per-request batch ceiling.
const BATCH: u32 = 8189;
/// An unreachable cluster never errors, the client retries forever; this is what makes it fail.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Account {
	id: String,
	user_data_128: String,
	user_data_64: u64,
	user_data_32: u32,
	ledger: u32,
	code: u16,
	flags: u16,
}

impl From<&tb::Account> for Account {
	fn from(a: &tb::Account) -> Self {
		Self {
			id: a.id.to_string(),
			user_data_128: a.user_data_128.to_string(),
			user_data_64: a.user_data_64,
			user_data_32: a.user_data_32,
			ledger: a.ledger,
			code: a.code,
			flags: a.flags.bits(),
		}
	}
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Transfer {
	id: String,
	debit_account_id: String,
	credit_account_id: String,
	amount: String,
	pending_id: String,
	user_data_128: String,
	user_data_64: u64,
	user_data_32: u32,
	timeout: u32,
	ledger: u32,
	code: u16,
	flags: u16,
}

impl From<&tb::Transfer> for Transfer {
	fn from(t: &tb::Transfer) -> Self {
		Self {
			id: t.id.to_string(),
			debit_account_id: t.debit_account_id.to_string(),
			credit_account_id: t.credit_account_id.to_string(),
			amount: t.amount.to_string(),
			pending_id: t.pending_id.to_string(),
			user_data_128: t.user_data_128.to_string(),
			user_data_64: t.user_data_64,
			user_data_32: t.user_data_32,
			timeout: t.timeout,
			ledger: t.ledger,
			code: t.code,
			flags: t.flags.bits(),
		}
	}
}

#[derive(Serialize, Deserialize)]
struct Gap {
	since: u64,
	accounts: Vec<Account>,
	transfers: Vec<Transfer>,
}

async fn bounded<T, E: std::fmt::Debug>(what: &str, request: Result<impl Future<Output = Result<T, E>>, tb::ClientClosed>) -> Result<T> {
	let request = request.map_err(|_| eyre!("{what}: client closed"))?;
	tokio::time::timeout(REQUEST_TIMEOUT, request)
		.await
		.map_err(|_| eyre!("{what}: no answer from the cluster within {REQUEST_TIMEOUT:?}"))?
		.map_err(|e| eyre!("{what}: {e:?}"))
}

fn filter(timestamp_min: u64, limit: u32, flags: tb::QueryFilterFlags) -> tb::QueryFilter {
	tb::QueryFilter {
		timestamp_min,
		limit,
		flags,
		..Default::default()
	}
}

/// The newest timestamp in the cluster, or the newest strictly below `before`.
async fn watermark(client: &tb::Client, before: Option<u64>) -> Result<u64> {
	let newest = tb::QueryFilter {
		timestamp_max: before.map_or(0, |b| b - 1),
		..filter(0, 1, tb::QueryFilterFlags::Reversed)
	};
	let transfer = bounded("query_transfers", client.query_transfers(newest)).await?.first().map_or(0, |t| t.timestamp);
	let account = bounded("query_accounts", client.query_accounts(newest)).await?.first().map_or(0, |a| a.timestamp);
	Ok(transfer.max(account))
}

async fn export(client: &tb::Client, since: u64) -> Result<Gap> {
	let mut gap = Gap {
		since,
		accounts: Vec::new(),
		transfers: Vec::new(),
	};
	let mut cursor = since;
	loop {
		let page = bounded("query_accounts", client.query_accounts(filter(cursor + 1, BATCH, tb::QueryFilterFlags::empty()))).await?;
		gap.accounts.extend(page.iter().map(Account::from));
		match page.last() {
			Some(last) if page.len() == BATCH as usize => cursor = last.timestamp,
			_ => break,
		}
	}
	cursor = since;
	loop {
		let page = bounded("query_transfers", client.query_transfers(filter(cursor + 1, BATCH, tb::QueryFilterFlags::empty()))).await?;
		gap.transfers.extend(page.iter().map(Transfer::from));
		match page.last() {
			Some(last) if page.len() == BATCH as usize => cursor = last.timestamp,
			_ => break,
		}
	}
	Ok(gap)
}

fn id(s: &str) -> Result<u128> {
	s.parse().wrap_err_with(|| format!("gap file id `{s}` is not a u128"))
}

/// `Ok(line)` when the live ledger holds every exported object unchanged, `Err(line)` otherwise.
async fn check(client: &tb::Client, gap: &Gap) -> Result<std::result::Result<String, String>> {
	let mut live_accounts = HashMap::new();
	for chunk in gap.accounts.chunks(BATCH as usize) {
		let ids = chunk.iter().map(|a| id(&a.id)).collect::<Result<Vec<_>>>()?;
		for a in bounded("lookup_accounts", client.lookup_accounts(&ids)).await? {
			live_accounts.insert(a.id.to_string(), Account::from(&a));
		}
	}
	let mut live_transfers = HashMap::new();
	for chunk in gap.transfers.chunks(BATCH as usize) {
		let ids = chunk.iter().map(|t| id(&t.id)).collect::<Result<Vec<_>>>()?;
		for t in bounded("lookup_transfers", client.lookup_transfers(&ids)).await? {
			live_transfers.insert(t.id.to_string(), Transfer::from(&t));
		}
	}

	let mut mismatched = 0;
	let missing_accounts = gap
		.accounts
		.iter()
		.filter(|a| match live_accounts.get(&a.id) {
			None => true,
			Some(live) => {
				mismatched += usize::from(live != *a);
				false
			}
		})
		.count();
	let mut missing_per_ledger: BTreeMap<u32, u128> = BTreeMap::new();
	let mut missing_transfers = 0;
	for t in &gap.transfers {
		match live_transfers.get(&t.id) {
			None => {
				missing_transfers += 1;
				*missing_per_ledger.entry(t.ledger).or_default() += t.amount.parse::<u128>().wrap_err("gap file amount is not a u128")?;
			}
			Some(live) => mismatched += usize::from(live != t),
		}
	}

	let since = jiff::Timestamp::from_nanosecond(i128::from(gap.since))
		.wrap_err("since is not a timestamp")?
		.strftime("%F %T UTC");
	if missing_transfers == 0 && missing_accounts == 0 && mismatched == 0 {
		return Ok(Ok(format!("all {} transfers and {} accounts since {since} present", gap.transfers.len(), gap.accounts.len())));
	}
	let sums = missing_per_ledger.iter().map(|(ledger, sum)| format!("{ledger}={sum}")).collect::<Vec<_>>().join(", ");
	Ok(Err(format!(
		"{missing_transfers} transfers (Σ amount per ledger: {sums}) and {missing_accounts} accounts missing, {mismatched} mismatched since {since}"
	)))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<ExitCode> {
	color_eyre::install()?;
	let mut command = None;
	let mut flags = HashMap::new();
	let mut args = std::env::args().skip(1);
	while let Some(arg) = args.next() {
		match arg.strip_prefix("--") {
			Some(name) => {
				let value = args.next().ok_or_else(|| eyre!("--{name} needs a value"))?;
				if flags.insert(name.to_owned(), value).is_some() {
					bail!("--{name} given twice");
				}
			}
			None if command.is_none() => command = Some(arg),
			None => bail!("unexpected argument `{arg}`"),
		}
	}
	let mut flag = |name: &str| flags.remove(name).ok_or_else(|| eyre!("--{name} is required"));
	let cluster_id: u128 = flag("cluster-id")?.parse().wrap_err("--cluster-id must be a u128")?;
	let tigerbeetle = TigerBeetle::connect(cluster_id, &flag("addresses")?)?;
	let client = tigerbeetle.client();

	let code = match command.as_deref() {
		Some("watermark") => {
			let before = flags
				.remove("before")
				.map(|b| b.parse::<u64>().wrap_err("--before must be a u64 TigerBeetle timestamp"))
				.transpose()?;
			if before == Some(0) {
				bail!("--before 0 bounds nothing below it");
			}
			println!("{}", watermark(client, before).await?);
			ExitCode::SUCCESS
		}
		Some("export") => {
			let since = flag("since")?.parse().wrap_err("--since must be a u64 TigerBeetle timestamp")?;
			let out = flag("out")?;
			let gap = export(client, since).await?;
			std::fs::write(&out, serde_json::to_vec(&gap)?).wrap_err_with(|| format!("write {out}"))?;
			println!("{} accounts and {} transfers since {since} → {out}", gap.accounts.len(), gap.transfers.len());
			ExitCode::SUCCESS
		}
		Some("check") => {
			let path = flag("gap")?;
			let gap: Gap = serde_json::from_slice(&std::fs::read(&path).wrap_err_with(|| format!("read {path}"))?).wrap_err_with(|| format!("parse {path}"))?;
			match check(client, &gap).await? {
				Ok(line) => {
					println!("{line}");
					ExitCode::SUCCESS
				}
				Err(line) => {
					println!("{line}");
					ExitCode::FAILURE
				}
			}
		}
		other => bail!("expected watermark | export | check, got {other:?}"),
	};
	if let Some(unused) = flags.keys().next() {
		bail!("--{unused} is not a flag of this command");
	}
	Ok(code)
}
