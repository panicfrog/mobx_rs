import SwiftCompilerPlugin
import SwiftSyntax
import SwiftSyntaxBuilder
import SwiftSyntaxMacros

@main
struct MobxSwiftPlugin: CompilerPlugin {
    let providingMacros: [Macro.Type] = [
        ActionMacro.self
    ]
}
