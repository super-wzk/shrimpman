{ inputs, ... }:
{
  imports = [ inputs.git-hooks.flakeModule ];

  perSystem =
    {
      config,
      lib,
      pkgs,
      ...
    }:
    let
      rust = pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
      rustfmt =
        workspace:
        let
          command = pkgs.writeShellApplication {
            name = "rustfmt-${workspace}";
            runtimeInputs = [ rust ];
            text = ''
              exec cargo fmt --manifest-path ${workspace}/Cargo.toml --all --check
            '';
          };
        in
        {
          enable = true;
          name = "rustfmt (${workspace})";
          package = command;
          entry = lib.getExe command;
          files = "^(${workspace}/.*\\.(rs|toml)|rust-toolchain\\.toml)$";
          pass_filenames = false;
        };
    in
    {
      pre-commit.settings = {
        package = pkgs.prek;
        hooks = {
          nixfmt = {
            enable = true;
            args = [ "--check" ];
          };
          rustfmt-shrimpman = rustfmt "shrimpman";
          rustfmt-mhf = rustfmt "mhf";
          check-merge-conflicts.enable = true;
          check-toml.enable = true;
        };
      };

      process-compose.shrimpman-dev.development = {
        packages = config.pre-commit.settings.enabledPackages;
        shellHook = config.pre-commit.shellHook;
      };
    };
}
