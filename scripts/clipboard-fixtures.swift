// Controlled native representations for check-clipboard.sh; never prints user bytes.
import AppKit
import Foundation

let args = CommandLine.arguments
let board = NSPasteboard.general
switch args[1] {
case "save":
    let items = (board.pasteboardItems ?? []).map { item in
        Dictionary(uniqueKeysWithValues: item.types.compactMap { type in
            item.data(forType: type).map { (type.rawValue, $0) }
        })
    }
    try PropertyListSerialization.data(fromPropertyList: items, format: .binary, options: 0)
        .write(to: URL(fileURLWithPath: args[2]), options: .atomic)
    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: args[2])
case "restore":
    let values = try PropertyListSerialization.propertyList(
        from: Data(contentsOf: URL(fileURLWithPath: args[2])), options: [], format: nil
    ) as! [[String: Data]]
    let items = values.map { value -> NSPasteboardItem in
        let item = NSPasteboardItem()
        for (type, data) in value {
            item.setData(data, forType: NSPasteboard.PasteboardType(type))
        }
        return item
    }
    board.clearContents()
    board.writeObjects(items)
case "types":
    print((board.types ?? []).map { $0.rawValue }.joined(separator: ", "))
case "offer":
    let png = try Data(contentsOf: URL(fileURLWithPath: args[2]))
    let rep = NSBitmapImageRep(data: png)!
    let item = NSPasteboardItem()
    for format in args[3].split(separator: ",") {
        switch format {
        case "png": item.setData(png, forType: .png)
        case "jpeg":
            item.setData(
                rep.representation(using: .jpeg, properties: [:])!,
                forType: NSPasteboard.PasteboardType("public.jpeg"))
        case "tiff": item.setData(rep.tiffRepresentation!, forType: .tiff)
        case "text": item.setString("clipboard fixture", forType: .string)
        default: fatalError("Unknown fixture format")
        }
    }
    board.clearContents()
    board.writeObjects([item])
default: fatalError("Unknown fixture operation")
}
