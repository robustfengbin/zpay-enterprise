# NU6.3 activation-day runbook (mainnet, 2026-07-28)

> Owner: jiaxu · written 2026-07-25 after the four transaction paths were proven
> on rail C. Read this before touching mainnet on activation day.
>
> **The residual risk this runbook exists for**: every post-NU6.3 path — turnstile
> crossing, Ironwood spend, shield, deshield — has been verified only on
> self-built test chains. That is by design (the standing rule is "green on a test
> chain before real money"), but it means **the first mainnet transaction after
> activation is still a first**. Step 3 is the only thing standing between that
> fact and customer funds.

## Before the day

| # | Check | How | Expected |
|---|---|---|---|
| 1 | Node knows NU6.3 | `getblockchaininfo` on the mainnet node | `upgrades[37a5165b] = { name: NU6.3, activationheight: 3428143, status: pending }` |
| 2 | Node version | `docker ps` on the host | zebra **6.2.1** (6.0.0 has no NU6.3 and stalls at activation) |
| 3 | Backend reads the height from the node | backend log at startup / a shielded proposal | era decisions must come from the node, never a constant |

Check 1 is the single point of failure: if the node does not report NU6.3, the
era decision stays PreNu63 forever and the service keeps building v5
transactions that consensus now rejects. Verified green 2026-07-25 (height
3,424,113, 4,030 blocks to go).

## On activation

1. **Confirm activation.** `getblockchaininfo` → `status: active` at ≥ 3,428,143.
2. **Do not batch anything yet.** Leave scheduled runs alone until step 3 passes.
3. **First crossing is a probe, with a small amount, from an internal wallet.**
   Never let the first post-activation transaction be customer funds. Migrate a
   token amount, wait for it to mine, and check all three:
   - transaction is **version 6** with an `orchard` bundle (the spend) and an
     `ironwood` bundle (the output);
   - the wallet's **legacy pool balance drops** and **Ironwood rises**;
   - the difference is exactly the fee — **20,000 zatoshis for a small crossing**,
     which is what a probe will be. See the fee table below before deciding the
     numbers disagree.
4. **Expect the first transaction to take ~20 s longer.** The PostNu6_3 proving
   key is built on first use. It is not a hang — do not restart the service, or
   it builds again from scratch.
5. **Then release the rest.** Batch runs and customer-facing shielding can
   proceed once step 3 reconciles.

## What a turnstile crossing costs

The fee follows the **action count**, not where the change goes. Post-NU6.3 the
old pool has cross-address transfers disabled, and in that mode the library
gives a spend and an output an action each (rather than pairing them), so the
old-pool bundle costs `spends + outputs` actions — padded up to a minimum of 2.
The Ironwood side is always 2. Every action is 5,000 zatoshis.

| Old-pool spends | Change kept in old pool | Actions (old + Ironwood) | Fee |
|---|---|---|---|
| 1 | no | 2 + 2 | **20,000** |
| 1 | yes | 2 + 2 | **20,000** — the extra output fits inside the padding |
| 2 | no | 2 + 2 | 20,000 |
| 2 | yes | 3 + 2 | 25,000 |
| 3 | yes | 4 + 2 | 30,000 |

A probe spends one note, so **expect 20,000** whether or not it retains change.
Reading "keeping change costs more" as a flat +5,000 would raise a false alarm
at exactly the wrong moment.

Source of truth: `turnstile_fee_zatoshis` in `orchard/turnstile.rs`, which calls
the same `BundleType::num_actions` the builder charges with; verified on rail C
(a 1-note batch retaining change paid 20,000).

## If something looks wrong

| Symptom | Most likely cause | Action |
|---|---|---|
| Shielded sends fail after activation | node not reporting NU6.3 (era stuck) | check 1 above; the era is read per call, so fixing the node is enough — no redeploy |
| A crossing is rejected by the mempool (-25) | fee shape vs actions mismatch | read the node's verbatim error from the item; do **not** hand-override the fee |
| First transaction seems stuck ~20 s | proving key build | wait; restarting makes it worse |
| Ironwood balance stays 0 after a crossing mines | scanner did not pick up the v6 bundle | run a scan; if still zero, capture the tx id and the scan log before changing anything |

## Invariants that must survive future changes

- **F4.2 CSV accepts Orchard unified addresses only.** The run-level fee headroom
  is 20,000 zatoshis per item; a deshield costs up to 25,000. Allowing
  t-addresses in a batch run without raising the headroom under-reserves by
  5,000 per item.
- **Anything migration-facing reads the legacy pool, never the cross-pool total.**
  The total counts funds already delivered into Ironwood, which makes the banner
  permanent and lets a plan be built over notes the turnstile builder refuses.
- **Non-final batches keep their change in the old pool** (`keeps_change_in_old_pool`).
  This is what makes a staggered private migration stagger at all; it depends on
  `seq` being 0-based, which both repositories guarantee via `enumerate()`.
