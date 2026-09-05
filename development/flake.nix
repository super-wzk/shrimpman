{ self, inputs, lib, ... }:
let
  inherit (inputs) nixpkgs rust-overlay process-compose-flake;
  # Git flake sources omit ignored files. --impure permits this optional
  # module to be read from the working tree instead of the Nix store.
  localRoot =
    let root = builtins.getEnv "PROJECT_ROOT";
    in if root != "" then root else builtins.getEnv "PWD";
  localModule = localRoot + "/local/default.nix";
  localModules = lib.optional
    (localRoot != "" && builtins.pathExists localModule)
    localModule;
  modules = [
    ./shell.nix
    ../shrimpman/default.nix
    ../mhf/default.nix
    ({ config, lib, ... }: {
      mhf.sign.http.base_url =
        lib.mkDefault "http://127.0.0.1:${toString config.development.ports.signHttp}";
    })
  ];
in
{
  imports = [ process-compose-flake.flakeModule ];

  perSystem = { config, pkgs, lib, self', system, ... }:
    let
      processGroup = config.process-compose.shrimpman-dev;
      # Public TOML generation must never include the local module.
      shared = ((import process-compose-flake.lib { inherit pkgs; }).evalModules {
        name = "shrimpman-dev";
        inherit modules;
      }).config;
      enterShell = name: command: pkgs.writeShellApplication {
        inherit name;
        runtimeInputs = [ pkgs.nix pkgs.git ];
        text = ''
          export PROJECT_ROOT="''${PROJECT_ROOT:-$(git rev-parse --show-toplevel)}"
          exec nix develop ${self}#devShells.${system}.default \
            ${lib.optionalString (localModules != [ ]) "--impure"} \
            --command ${lib.getExe command} "$@"
        '';
      };
      dev = {
        program = enterShell "shrimpman-dev" processGroup.outputs.package;
        meta.description = "Run the development process group inside the devShell.";
      };
    in
    {
      _module.args.pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };

      process-compose.shrimpman-dev.imports = modules ++ localModules;
      devShells.default = processGroup.outputs.devShell;

      packages = processGroup.development.commands // {
        update-configs = pkgs.writeShellApplication {
          name = "update-configs";
          runtimeInputs = [ pkgs.git pkgs.coreutils ];
          text = ''
            cd "$(git rev-parse --show-toplevel)"
            ${lib.concatStringsSep "\n" (lib.mapAttrsToList (name: path: ''
              cp ${path} ${lib.escapeShellArg name}
              chmod u+w ${lib.escapeShellArg name}
            '') shared.generatedFiles)}
          '';
        };
      };

      apps = lib.mapAttrs (name: command: {
        program = enterShell name command;
        meta.description = command.meta.description or "Run ${name} inside the devShell.";
      }) processGroup.development.commands // {
        inherit dev;
        default = dev;
        # process-compose-flake exports the raw package under this name.
        # Its nix run entry must also enter the development shell.
        shrimpman-dev = dev;
        update-configs = {
          program = lib.getExe self'.packages.update-configs;
          meta.description = "Regenerate public TOML defaults without local overrides.";
        };
      };

      checks.generated-configs = pkgs.runCommand "check-generated-configs" { } ''
        ${lib.concatStringsSep "\n" (lib.mapAttrsToList (name: path:
          "diff -u ${../. + "/${name}"} ${path}"
        ) shared.generatedFiles)}
        touch "$out"
      '';
    };
}
