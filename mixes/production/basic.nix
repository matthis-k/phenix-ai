{ lib }:
spec:
(import ./native-only.nix { inherit lib; }) "Basic" spec
