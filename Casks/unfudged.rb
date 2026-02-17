cask "unfudged" do
  version "0.16.2"
  sha256 "6277bf1ca98d2052099af28f8be4dea3ba9a84a164b28c08e827398716f57543"

  url "https://github.com/cyrusradfar/homebrew-unf/releases/download/v0.16.2/UNFUDGED-v0.16.2-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://github.com/cyrusradfar/unfudged"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
