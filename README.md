# java-wis

Extension Java nhẹ cho WiSCode (dùng `zed_extension_api`, nên cũng chạy được trên Zed).

## Tính năng

- **Java**: tô màu cú pháp, outline/breadcrumbs, text objects, tự thụt lề.
- **Nút ▶ trong gutter** (giống IntelliJ):
  - cạnh `public static void main(...)`: menu có `run: <Class>` và `debug: <Class>`; cả hai chạy qua
    java-debug với classpath do jdtls tính (giống IntelliJ), `run` là chạy không debug
  - cạnh method `@Test` / `@ParameterizedTest` / `@RepeatedTest`: chạy test bằng Maven hoặc Gradle
- **Eclipse JDT Language Server (jdtls)**: completion, go to definition, **go to implementation**
  (kết hợp với icon "implementation" mới trong gutter của WiSCode), rename, diagnostics, hỗ trợ Maven/Gradle/Lombok.
  - Dùng `jdtls` có sẵn trong `PATH` nếu có; nếu không, tự tải bản mới nhất từ download.eclipse.org.
  - Cần **JDK 21+** (`JAVA_HOME` hoặc `java` trong `PATH`).
  - Mỗi project có một thư mục workspace jdtls riêng.
- **Debug** (adapter `Java`: [java-debug](https://github.com/microsoft/java-debug) chạy trong jdtls):
  - Breakpoint, step, xem biến như mọi debugger khác của WiSCode.
  - Có thể khai báo trong `.wu/debug.json` (một mảng), ví dụ:

    ```json
    [
      { "adapter": "Java", "label": "Main", "request": "launch", "mainClass": "com.example.Main" },
      { "adapter": "Java", "label": "Attach", "request": "attach", "hostName": "localhost", "port": 5005 }
    ]
    ```
  - Lần đầu sẽ tải `java-lsp-proxy` và plugin java-debug vào thư mục work của extension.
- **Java Properties** (`application.properties` của Quarkus/Spring): tô màu, outline, `${ENV:default}`.
- **Run configurations trên thanh tiêu đề** (khai báo trong `run_configurations/java.json`, WiSCode không fix cứng):
  - Maven: clean install, package, compile, test, install (skip tests)
  - Gradle: build, clean build, test, run
  - Quarkus (Maven/Gradle): dev, test, build, build native
  - Spring Boot (Maven/Gradle): run / bootRun
  - Maven profiles: các `<profile><id>` trong `pom.xml` hiện thành checkbox ở tab Run configurations bên trái;
    profile được tick sẽ thêm `-P<id1>,<id2>` vào mọi lệnh Maven (lưu riêng cho từng project).
  - Nút **Debug** (cạnh nút Run trên thanh tiêu đề và trong tab Run configurations) cho:
    Spring Boot run/bootRun, Quarkus dev, Maven test, Gradle run/test. Lệnh chạy kèm JDWP ở cổng 5005,
    rồi debugger tự attach khi cổng mở.
  - Thứ tự chọn lệnh: `mvnw`/`gradlew` của project → `mvn`/`gradle` trong `PATH` → bản do extension tự cài.
- **Tự cài công cụ** (vào thư mục work của extension, chỉ khi máy chưa có):
  - Apache Maven bản ổn định mới nhất (khi có `pom.xml` mà không có `mvnw` hay `mvn`)
  - Gradle bản mới nhất (khi có `build.gradle(.kts)` mà không có `gradlew` hay `gradle`)
  - Lombok (gắn vào jdtls bằng `-javaagent`)
  - Việc cài chạy khi jdtls khởi động, tức là lần đầu mở file `.java`.

## Cài đặt (dev extension)

1. Cài Rust qua rustup (file `rust-toolchain.toml` sẽ tự cài target `wasm32-wasip2`).
2. Trong WiSCode: Command Palette → `extensions: install dev extension` → chọn thư mục `java-wis`.

## Cấu hình

```jsonc
// settings.json
{
  "lsp": {
    "jdtls": {
      // dùng jdtls tự cài
      "binary": { "path": "C:/tools/jdtls/bin/jdtls.bat", "arguments": [] },
      // gộp vào settings mặc định gửi cho jdtls
      "settings": {
        "java": {
          "configuration": {
            // chọn JDK: bản có "default": true được jdtls dùng để biên dịch/phân tích,
            // và được đặt làm JAVA_HOME cho các run configuration Maven/Gradle
            "runtimes": [
              { "name": "JavaSE-17", "path": "C:/Program Files/Java/jdk-17" },
              { "name": "JavaSE-21", "path": "C:/Program Files/Java/jdk-21", "default": true }
            ]
          }
        }
      }
    }
  }
}
```

- `runtimes` có thể đặt riêng cho từng project trong `.wu/settings.json` của project đó.
- Bản thân jdtls vẫn chạy bằng JDK 21+ lấy từ `JAVA_HOME`/`PATH` của máy.
- Nút ▶ cạnh `@Test` trong gutter chưa dùng JDK chọn ở đây.
- Debug JUnit test từ gutter chưa hỗ trợ; dùng nút Debug của run configuration `test`.

## Giấy phép

Apache-2.0. `src/debugger.rs` và `debug_adapter_schemas/Java.json` dựa trên
[zed-extensions/java](https://github.com/zed-extensions/java) (Apache-2.0).
