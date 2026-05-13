; JavaScript edge extraction queries

; Function calls
(call_expression function: (identifier) @call.name)
(call_expression function: (member_expression property: (property_identifier) @call.name))

; Import statements
(import_statement source: (string) @import.path)

; Require calls
(call_expression
  function: (identifier) @_fn
  arguments: (arguments (string) @import.path)
  (#eq? @_fn "require"))
