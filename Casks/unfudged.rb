cask "unfudged" do
  version "0.17.16"
  sha256 "6691e44ff9de86008eed86fcf59a27d791fd07e11cd0021d1781610cdb53ca2f"

  url "https://downloads.unfudged.io/releases/v0.17.16/UNFUDGED-v0.17.16-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
