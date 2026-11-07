; Rust symbol extraction queries
; Captures: @symbol.name @symbol.def @symbol.type (declared/return type)

; Functions and methods (methods are detected from the enclosing impl/trait)
(function_item name: (identifier) @symbol.name return_type: (_)? @symbol.type) @symbol.def

; Trait method signatures
(function_signature_item name: (identifier) @symbol.name return_type: (_)? @symbol.type) @symbol.def

; Structs
(struct_item name: (type_identifier) @symbol.name) @symbol.def

; Struct fields
(field_declaration name: (field_identifier) @symbol.name type: (_) @symbol.type) @symbol.def

; Enums and variants
(enum_item name: (type_identifier) @symbol.name) @symbol.def
(enum_variant name: (identifier) @symbol.name) @symbol.def

; Unions
(union_item name: (type_identifier) @symbol.name) @symbol.def

; Traits
(trait_item name: (type_identifier) @symbol.name) @symbol.def

; Impl blocks (named after the implementing type)
(impl_item type: [
  (type_identifier) @symbol.name
  (generic_type type: (type_identifier) @symbol.name)
  (scoped_type_identifier name: (type_identifier) @symbol.name)
]) @symbol.def

; Type aliases
(type_item name: (type_identifier) @symbol.name) @symbol.def

; Constants and statics
(const_item name: (identifier) @symbol.name type: (_) @symbol.type) @symbol.def
(static_item name: (identifier) @symbol.name type: (_) @symbol.type) @symbol.def

; Modules
(mod_item name: (identifier) @symbol.name) @symbol.def

; Macro definitions
(macro_definition name: (identifier) @symbol.name) @symbol.def
