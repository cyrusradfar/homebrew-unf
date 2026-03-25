cask "unfudged" do
  version "0.17.15"
  sha256 "7961b56f3d0b93f09816db4aade82ccc8168c8da3425cb0ce7bc0435055abe21"

  url "https://downloads.unfudged.io/releases/v0.17.15/UNFUDGED-v0.17.15-universal.dmg"
  name "UNFUDGED"
  desc "High-resolution filesystem flight recorder - desktop app"
  homepage "https://unfudged.io"

  depends_on formula: "cyrusradfar/unf/unf"
  depends_on macos: ">= :catalina"

  app "UNFUDGED.app"
end
