{
  system ? builtins.currentSystem,
}:
(import ./nix { inherit system; }).no-js-speedtest
