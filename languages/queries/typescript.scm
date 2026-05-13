; TypeScript symbol extraction queries

; Function declarations
(function_declaration name: (identifier) @symbol.name) @symbol.def

; Class declarations
(class_declaration name: (type_identifier) @symbol.name) @symbol.def

; Interface declarations
(interface_declaration name: (type_identifier) @symbol.name) @symbol.def

; Type aliases
(type_alias_declaration name: (type_identifier) @symbol.name) @symbol.def

; Enum declarations
(enum_declaration name: (identifier) @symbol.name) @symbol.def

; Arrow functions assigned to variables
(lexical_declaration (variable_declarator name: (identifier) @symbol.name value: (arrow_function))) @symbol.def

; Method definitions in classes
(method_definition name: (property_identifier) @symbol.name) @symbol.def

; Exported functions/classes
(export_statement declaration: (function_declaration name: (identifier) @symbol.name)) @symbol.def
(export_statement declaration: (class_declaration name: (type_identifier) @symbol.name)) @symbol.def
