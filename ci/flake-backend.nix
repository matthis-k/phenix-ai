# The execution policy is separate from the consumer's Nix check definitions.
# The remote backend intentionally uses the benchmark-selected defaults:
# four workers and two build slots, with Tailscale OIDC credentials.
{
  backend = "nix-remote";
  remote = {
    transport = "tailscale";
    credentials = "oidc";
    workers = 4;
    slotsPerWorker = 2;
  };
}
