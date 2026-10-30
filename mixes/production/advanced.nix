{ lib }:
spec:
(import ./native-only.nix { inherit lib; }) "Advanced" spec
