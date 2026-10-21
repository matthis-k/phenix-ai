# B2 matched-run probe (temporary)

This file is identical in both disposable CI benchmark branches. It is not
part of Nix or Cargo source inputs and changes no check, runtime or plugin.
The only intended configuration difference between the benchmark arms is
whether `phenix-provider-contracts` belongs to the shared prebuild hotset.
Record measured workflow timestamps and cache evidence before cleanup.
