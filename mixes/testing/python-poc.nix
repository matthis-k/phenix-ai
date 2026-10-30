# Standalone source fixture, never selected as a Basic/Advanced artifact.
{ source }:
{
  kind = "standalone-experiment";
  inherit source;
  production = false;
}
