pkg:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.mev-rs;

  component = cfg.enable;
  # ensure the intended component is part of the feature set
  features = if lib.strings.hasInfix component cfg.features then cfg.features else lib.strings.removePrefix "," "${cfg.features},${component}";

  mev-rs = pkg.mev-rs {
    inherit features;
    system = pkgs.system;
  };

  name = "mev-${cfg.enable}-rs";

  cmd = ''
    ${mev-rs}/bin/mev \
    ${component} \
    ${cfg.config-file}
  '';
in
{
  options.services.mev-rs = {
    enable = lib.mkOption {
      type = lib.types.enum [ "boost" "build" ];
      description = "which subcommand of `mev-rs` to run";
    };
    config-file = lib.mkOption {
      type = lib.types.str;
      description = ''
        path to a config file suitable for the `mev-rs` toolkit
      '';
    };
    features = lib.mkOption {
      type = lib.types.str;
      default = cfg.enable;
      description = ''
        feature set (comma-separated) to enable for `cargo` build
      '';
    };
  };

  config = {
    networking.firewall = lib.mkIf (component == "build") {
      allowedTCPPorts = [ 30303 ];
      allowedUDPPorts = [ 30303 ];
    };

    environment.systemPackages = [
      mev-rs
    ];

    systemd.services."${name}" = {
      description = name;
      wantedBy = [ "multi-user.target" ];
      after = [ "vc.service" ];
      serviceConfig = {
        ExecStart = cmd;
        Restart = "on-failure";
        RestartSec = "10s";
        SyslogIdentifier = name;
      };
    };
  };
}
