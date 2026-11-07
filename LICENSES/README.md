# 第三方依赖清单

`dependency-inventory.json` 绑定三个锁文件的 SHA256，记录依赖精确版本、许可证元数据及可取得的许可证正文。包括构建和测试依赖，不等于安装包实际包含全部依赖。

生成命令：`source scripts/env.sh && uv run --project worker python scripts/licenses.py`。

未附独立许可证正文的包需要正式发布前复核（部分包将许可证写在 README 或元数据中）：

- cargo / alloc-stdlib 0.3.0
- cargo / block2 0.6.2
- cargo / cesu8 1.1.0
- cargo / dispatch2 0.3.1
- cargo / dlopen2 0.8.2
- cargo / dlopen2_derive 0.4.3
- cargo / gloo-timers 0.4.0
- cargo / jni 0.22.4
- cargo / jni-macros 0.22.4
- cargo / jni-sys-macros 0.4.1
- cargo / libappindicator-sys 0.9.0
- cargo / ndk 0.9.0
- cargo / ndk-context 0.1.1
- cargo / ndk-sys 0.6.0+11769913
- cargo / objc2 0.6.5
- cargo / objc2-app-kit 0.3.2
- cargo / objc2-cloud-kit 0.3.2
- cargo / objc2-core-data 0.3.2
- cargo / objc2-core-foundation 0.3.2
- cargo / objc2-core-graphics 0.3.2
- cargo / objc2-core-image 0.3.2
- cargo / objc2-core-location 0.3.2
- cargo / objc2-core-text 0.3.2
- cargo / objc2-encode 4.1.0
- cargo / objc2-exception-helper 0.1.1
- cargo / objc2-foundation 0.3.2
- cargo / objc2-io-surface 0.3.2
- cargo / objc2-quartz-core 0.3.2
- cargo / objc2-ui-kit 0.3.2
- cargo / objc2-user-notifications 0.3.2
- cargo / objc2-web-kit 0.3.2
- cargo / r-efi 5.3.0
- cargo / r-efi 6.0.0
- cargo / rsqlite-vfs 0.1.1
- cargo / rustls-platform-verifier-android 0.2.0
- cargo / selectors 0.38.0
- cargo / webview2-com 0.39.1
- cargo / webview2-com-macros 0.8.1
- cargo / webview2-com-sys 0.39.1
- cargo / winapi-i686-pc-windows-gnu 0.4.0
- cargo / winapi-x86_64-pc-windows-gnu 0.4.0
- pnpm / @esbuild/linux-x64 0.28.2
- pnpm / @rollup/rollup-linux-x64-gnu 4.65.0
- pnpm / @napi-rs/lzma-linux-x64-gnu 1.5.1
- uv / pyserial 3.5
