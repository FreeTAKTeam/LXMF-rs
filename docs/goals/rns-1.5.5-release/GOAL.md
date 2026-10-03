# Goal: release LXMF-rs v0.13.0 with Reticulum 1.5.5 feature updates

Execute [PLAN.md](PLAN.md) on one isolated checkout from current main. Keep the tested baseline and public contracts intact, implement the changed 1.5.5 features on existing paths, and prove the affected behavior against the exact Python tag. Do not claim blanket Reticulum parity or require #616 physical/client/public-network testing. Publish only after local gates and exact-head CI pass, then independently verify the public release.
