pkg:
{ config, lib, pkgs, ... }:
let
  mev-rs-with-features = features: pkg.mev-rs {
    inherit features;
    system = pkgs.system;
  };

  mev-rs-submodule = for-component: { config, ... }: with lib; with types;
    let
      features = strings.concatStringsSep "," [ for-component ] ++ config.additional-features;
      mev-rs = mev-rs-with-features features;
      name = "mev-${for-component}-rs";
      cmd = ''
        ${mev-rs}/bin/mev \
        ${for-component} \
        ${config.config-file}
      '';
    in
    {
      options = {
        component = mkOption {
          type = enum [ "boost" "relay" "build" ];
          default = for-component;
        };
        config-file = mkOption {
          type = str;
          description = ''
            path to a config file suitable for the `mev-rs` toolkit
          '';
        };
        additional-features = mkOption {
          type = listOf str;
          description = ''
            additional Cargo features to include alongside those required for the `component`
          '';
        };
        after-systemd-service = mkOption {
          type = str;
          default = "vc.service";
          description = ''
            the name of a systemd service to wait activation of before activating this unit
          '';
        };
      };
      config = {
        environment.systemPackages = [
          mev-rs
        ];
        networking.firewall = lib.mkIf (for-component == "build") {
          allowedTCPPorts = [ 30303 ];
          allowedUDPPorts = [ 30303 ];
        };

        systemd.services."${name}" = {
          description = name;
          wantedBy = [ "multi-user.target" ];
          after = [ config.after-systemd-service ];
          serviceConfig = {
            ExecStart = cmd;
            Restart = "on-failure";
            RestartSec = "10s";
            SyslogIdentifier = for-component;
          };
        };
      };
    };
in
{
  options.services.mev-rs = with lib; with types; mkOption {
    type = listOf (submodule {
      options.boost = mkOption {
        type = attrsOf (submodule mev-rs-submodule "boost");
      };
      options.relay = mkOption {
        type = attrsOf (submodule mev-rs-submodule "relay");
      };
      options.build = mkOption {
        type = attrsOf (submodule mev-rs-submodule "build");
      };
    });
  };
}
