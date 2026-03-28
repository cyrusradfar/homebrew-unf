cask "unfudged" do
  version "0.17.16"
  sha256 "99b2a3d6565780bb4911e2a636955b303867c7e8bd14b153d18d90e76e226cc8"

  url "https://downloads.unfudged.io/releases/v0.17.16/UNFUDGED-v0.17.16-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
