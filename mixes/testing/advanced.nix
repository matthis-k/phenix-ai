# Advanced-domain verification without adding test plugins to the preset.
{
  kind = "native-check-domain";
  checks = [
    "p2-model-adapter-product"
    "p2-compaction-checkpoints-product"
    "p2-evidence-store-product"
  ];
}
