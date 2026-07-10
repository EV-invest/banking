# Prod TigerBeetle cluster identity — THE source of truth. The rpi5 nix module
# (~/nix hosts/rpi5/tigerbeetle.nix) mirrors these values with a comment saying
# so; prod TIGERBEETLE_ADDRESS (CSV of `addresses`) / TIGERBEETLE_CLUSTER_ID
# derive from here when banking gets a deployment contract.
#
# `addresses` is index-ordered: entry i is replica i, positionally — TB
# resolves replicas by index into this exact list. Changing an entry = edit
# BOTH files + rebuild rpi5 + rolling restart (`nix run .#new-replica` when
# the change IS a replica move). replica count is frozen forever by TB.
{
  clusterId = "292435992253610338193003275140236465861";
  addresses = [ "100.91.243.126:3001" "100.91.243.126:3002" "100.91.243.126:3003" ];
}
