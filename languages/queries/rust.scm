; Rust symbol extraction queries

; Functions
(function_item name: (identifier) @symbol.name) @symbol.def

; Structs
(struct_item name: (type_identifier) @symbol.name) @symbol.def

; Enums
(enum_item name: (type_identifier) @symbol.name) @symbol.def

; Traits
(trait_item name: (type_identifier) @symbol.name) @symbol.def

; Impl blocks
(impl_item type: (type_identifier) @symbol.name) @symbol.def

; Type aliases
(type_item name: (type_identifier) @symbol.name) @symbol.def

; Constants
(const_item name: (identifier) @symbol.name) @symbol.def

; Static items
(static_item name: (identifier) @symbol.name) @symbol.def

; Modules
(mod_item name: (identifier) @symbol.name) @symbol.def
