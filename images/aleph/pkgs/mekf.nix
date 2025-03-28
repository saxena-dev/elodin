{
  pkgs,
  crane,
  ...
}: let
  rustToolchain = p: p.rust-bin.stable."1.85.0".default;
  craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
  crateName = craneLib.crateNameFromCargoToml {cargoToml = ../../../fsw/mekf/Cargo.toml;};
  src = pkgs.nix-gitignore.gitignoreSource [] ../../../.;
  commonArgs = {
    inherit (crateName) pname;
    inherit src;
    version = "0.12.0";
    doCheck = false;
    cargoExtraArgs = "--package=${crateName.pname}";
    HOST_CC = "${pkgs.stdenv.cc.nativePrefix}cc";
    TARGET_CC = "${pkgs.stdenv.cc.targetPrefix}cc";
    buildInputs = [
      pkgs.buildPackages.clang
    ];
    LIBCLANG_PATH = "${pkgs.buildPackages.libclang.lib}/lib";
  };
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
  bin = craneLib.buildPackage (commonArgs
    // {
      inherit cargoArtifacts;
    });
in
  bin
