{ pkgs ? import ../../nix { inherit system; }
, dfx ? import ../../dfx.nix { inherit pkgs; }
, system ? builtins.currentSystem
, use_ic_ref ? false
, assets
, utils
}:
let
  inherit (pkgs) lib;

  isBatsTest = fileName: type: lib.hasSuffix ".bash" fileName && type == "regular";

  here = ./.;

  mkBatsTest = fileName:
    let
      name = lib.removeSuffix ".bash" fileName;
    in
      lib.nameValuePair name (
        pkgs.runCommandNoCC "e2e-replica-test-${name}${lib.optionalString use_ic_ref "-use_ic_ref"}" {
          nativeBuildInputs = with pkgs; [
            bats
            diffutils
            curl
            findutils
            gnugrep
            gnutar
            gzip
            jq
            mitmproxy
            netcat
            nodejs
            ps
            python3
            procps
            which
            dfx.standalone
          ] ++ lib.optional use_ic_ref ic-ref;
          BATSLIB = pkgs.sources.bats-support;
          USE_IC_REF = use_ic_ref;
          assets = assets;
          utils = utils;
          test = here + "/${fileName}";
        } ''
          export HOME=$(pwd)

          ln -s $utils utils
          ln -s $assets assets
          mkdir test
          ln -s $test test/test.bash

          # Timeout of 10 minutes is enough for now. Reminder; CI might be running with
          # less resources than a dev's computer, so e2e might take longer.
          timeout --preserve-status 3600 bats test/test.bash | tee $out
        ''
      );
in
builtins.listToAttrs
  (
    builtins.map mkBatsTest
      (
        lib.attrNames
          (
            lib.filterAttrs isBatsTest
              (builtins.readDir here)
          )
      )
  ) // { recurseForDerivations = true; }
