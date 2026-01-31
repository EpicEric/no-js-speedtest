{
  description = "Speedtest webapp that doesn't use any JavaScript";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        rustChannel = "stable";
        rustVersion = "latest";

        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        inherit (pkgs) lib;

        craneLib = (crane.mkLib pkgs).overrideToolchain (
          pkgs: pkgs.rust-bin.${rustChannel}.${rustVersion}.default
        );

        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            (craneLib.fileset.commonCargoSources ./.)
            ./src/favicon.png
            ./templates
          ];
        };

        commonArgs = {
          inherit src;
          strictDeps = true;

          nativeBuildInputs = [
            pkgs.cmake
            pkgs.perl
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        no-js-speedtest =
          (craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              doCheck = false;
            }
          ))
          // {
            meta.mainProgram = "no-js-speedtest";
          };
      in
      {
        packages = {
          inherit no-js-speedtest;
          default = no-js-speedtest;
        };

        apps.default = (
          flake-utils.lib.mkApp {
            drv = no-js-speedtest;
          }
        );

        checks = {
          inherit no-js-speedtest;

          no-js-speedtest-clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
            }
          );

          no-js-speedtest-doc = craneLib.cargoDoc (
            commonArgs
            // {
              inherit cargoArtifacts;
            }
          );

          no-js-speedtest-fmt = craneLib.cargoFmt {
            inherit src;
          };
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          inputsFrom = [ no-js-speedtest ];

          packages = [
            pkgs.bacon
          ];
        };
      }
    );
}
