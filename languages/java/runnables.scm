; public static void main(String[] args)
(
  (program
    (package_declaration
      (_) @java_package_name)?
    (class_declaration
      name: (identifier) @java_class_name
      body: (class_body
        (method_declaration
          (modifiers) @_modifiers
          type: (void_type)
          name: (identifier) @run @_main
          (#eq? @_main "main")
          (#match? @_modifiers "static")))))
  @_java-main
  (#set! tag java-main))

; JUnit test method
(
  (program
    (package_declaration
      (_) @java_package_name)?
    (class_declaration
      name: (identifier) @java_class_name
      body: (class_body
        (method_declaration
          (modifiers
            [
              (marker_annotation name: (identifier) @_annotation)
              (annotation name: (identifier) @_annotation)
            ])
          name: (identifier) @run @java_method_name
          (#any-of? @_annotation "Test" "ParameterizedTest" "RepeatedTest")) @_java-test-method)))
  (#set! tag java-test-method))
