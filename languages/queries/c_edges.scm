; C edge extraction queries

; Function calls
(call_expression function: (identifier) @call.name)
(call_expression function: (field_expression field: (field_identifier) @call.name))

; Include directives
(preproc_include path: (string_literal) @import.path)
(preproc_include path: (system_lib_string) @import.path)
