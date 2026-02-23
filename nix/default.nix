{
  system ? builtins.currentSystem,
  rustChannel ? "stable",
  rustVersion ? "latest",
}:
let
  sources = import ../npins;

  pkgs = import sources.nixpkgs {
    inherit system;
    overlays = [ (import sources.rust-overlay) ];
  };

  inherit (pkgs) lib;

  craneLib = (import sources.crane { inherit pkgs; }).overrideToolchain (
    p: p.rust-bin.${rustChannel}.${rustVersion}.default
  );

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      (craneLib.fileset.commonCargoSources ../.)
      ../src/favicon.png
      ../templates
    ];
  };

  commonArgs = {
    inherit src;
    strictDeps = true;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  no-js-speedtest = craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts;
      doCheck = false;
      meta.mainProgram = "no-js-speedtest";
    }
  );
in
{
  inherit pkgs no-js-speedtest;

  packages = {
    inherit no-js-speedtest;
    default = no-js-speedtest;
  };

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

  shell = craneLib.devShell {
    packages = [
      pkgs.bacon
    ];
  };
}
