# Storage and project membership test domain.
{
  kind = "native-check-domain";
  checks = [
    "b2-sqlite"
    "p2-project-storage-product"
    "p2-project-membership-product"
    "p2-project-source-product"
  ];
}
