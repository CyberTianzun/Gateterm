import Foundation
import SwiftUI
import UIKit

private let mdMaxDepth = 16
private let mdCopyFeedbackMs: UInt64 = 1500

private final class CachedMdDocument: @unchecked Sendable {
    let doc: FfiMdDocument
    init(_ doc: FfiMdDocument) { self.doc = doc }
}

private enum MarkdownCache {
    private static let cache: NSCache<NSString, CachedMdDocument> = {
        let cache = NSCache<NSString, CachedMdDocument>()
        cache.countLimit = 256
        return cache
    }()

    static func parse(_ text: String) -> FfiMdDocument {
        let key = text as NSString
        if let cached = cache.object(forKey: key) {
            return cached.doc
        }
        let doc = parseMarkdown(text: text)
        cache.setObject(CachedMdDocument(doc), forKey: key)
        return doc
    }
}

func buildMarkdownAttributedString(
    runs: [FfiMdRun],
    palette: UiPalette,
    baseSize: CGFloat,
    baseBold: Bool = false,
    strikeThrough: Bool = false
) -> AttributedString {
    var result = AttributedString()
    for run in runs {
        guard !run.text.isEmpty else { continue }
        var segment = AttributedString(run.text)
        let fontSize = run.code ? baseSize * 0.94 : baseSize
        let isBold = baseBold || run.bold
        let baseFont = Font.system(size: fontSize, weight: isBold ? .bold : .regular, design: .monospaced)
        segment.swiftUI.font = run.italic ? baseFont.italic() : baseFont

        if let url = sanitizeMarkdownLinkUrl(run.link) {
            segment.link = url
            segment.swiftUI.foregroundColor = Color(rgb: palette.accent)
            segment.swiftUI.underlineStyle = .single
        }
        if run.code {
            segment.swiftUI.foregroundColor = Color(rgb: palette.mdCode)
            segment.swiftUI.backgroundColor = Color(rgb: palette.panel2)
        }
        if run.strikethrough || strikeThrough {
            segment.swiftUI.strikethroughStyle = .single
        }
        result.append(segment)
    }
    return result
}

struct ChatMarkdownView: View {
    let text: String
    let palette: UiPalette

    var body: some View {
        let doc = MarkdownCache.parse(text)
        if !doc.root.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                MarkdownBlockListView(
                    doc: doc,
                    indices: doc.root,
                    palette: palette,
                    depth: 0,
                    listDepth: 0,
                    strikeThrough: false
                )
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct MarkdownBlockListView: View {
    let doc: FfiMdDocument
    let indices: [UInt32]
    let palette: UiPalette
    let depth: Int
    let listDepth: Int
    let strikeThrough: Bool

    @ViewBuilder
    var body: some View {
        if depth >= mdMaxDepth - 1 {
            MarkdownFlattenedBlockListView(
                doc: doc,
                indices: indices,
                palette: palette,
                strikeThrough: strikeThrough
            )
        } else {
            ForEach(Array(indices.enumerated()), id: \.offset) { _, idx in
                let blockIndex = Int(idx)
                if doc.blocks.indices.contains(blockIndex) {
                    MarkdownBlockView(
                        doc: doc,
                        block: doc.blocks[blockIndex],
                        palette: palette,
                        depth: depth,
                        listDepth: listDepth,
                        strikeThrough: strikeThrough
                    )
                }
            }
        }
    }
}

private struct MarkdownFlattenedBlockListView: View {
    let doc: FfiMdDocument
    let indices: [UInt32]
    let palette: UiPalette
    let strikeThrough: Bool

    var body: some View {
        let flat = flattenDeepMarkdownBlocks(
            indices: indices,
            in: doc,
            inheritedStrikeThrough: strikeThrough
        )
        ForEach(Array(flat.enumerated()), id: \.offset) { _, entry in
            MarkdownLeafBlockView(
                block: doc.blocks[entry.blockIndex],
                palette: palette,
                strikeThrough: entry.strikeThrough
            )
        }
    }
}

private struct MarkdownLeafBlockView: View {
    let block: FfiMdBlock
    let palette: UiPalette
    let strikeThrough: Bool

    @ViewBuilder
    var body: some View {
        switch block {
        case let .heading(level, runs):
            MarkdownHeadingView(level: Int(level), runs: runs, palette: palette, strikeThrough: strikeThrough)
        case let .paragraph(runs):
            MarkdownParagraphView(runs: runs, palette: palette, strikeThrough: strikeThrough)
        case let .codeBlock(lang, code):
            MarkdownCodeBlockView(lang: lang, code: code, palette: palette)
        case let .table(alignments, headers, rows):
            MarkdownTableView(alignments: alignments, headers: headers, rows: rows, palette: palette)
        case .thematicBreak:
            Rectangle()
                .fill(Color(rgb: palette.line))
                .frame(height: 1)
                .padding(.vertical, 4)
        case .blockQuote, .listBlock:
            EmptyView()
        }
    }
}

private struct MarkdownBlockView: View {
    let doc: FfiMdDocument
    let block: FfiMdBlock
    let palette: UiPalette
    let depth: Int
    let listDepth: Int
    let strikeThrough: Bool

    @ViewBuilder
    var body: some View {
        switch block {
        case .heading, .paragraph, .codeBlock, .table, .thematicBreak:
            MarkdownLeafBlockView(block: block, palette: palette, strikeThrough: strikeThrough)
        case let .blockQuote(children):
            MarkdownBlockQuoteView(
                doc: doc,
                children: children,
                palette: palette,
                depth: depth,
                listDepth: listDepth,
                strikeThrough: strikeThrough
            )
        case let .listBlock(ordered, start, items):
            MarkdownListView(
                doc: doc,
                ordered: ordered,
                start: start,
                items: items,
                palette: palette,
                depth: depth,
                listDepth: listDepth,
                strikeThrough: strikeThrough
            )
        }
    }
}

private struct MarkdownHeadingView: View {
    let level: Int
    let runs: [FfiMdRun]
    let palette: UiPalette
    let strikeThrough: Bool

    var body: some View {
        let clamped = min(6, max(1, level))
        let size = CGFloat(markdownHeadingFontSize(clamped))
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 0) {
                Text(String(repeating: "#", count: clamped) + " ")
                    .font(.system(size: size, weight: .regular, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.dim))
                Text(buildMarkdownAttributedString(
                    runs: runs,
                    palette: palette,
                    baseSize: size,
                    baseBold: true,
                    strikeThrough: strikeThrough
                ))
                .foregroundStyle(Color(rgb: palette.mdHeading))
                .tint(Color(rgb: palette.accent))
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            if clamped <= 2 {
                Rectangle()
                    .fill(Color(rgb: palette.line))
                    .frame(height: 1)
            }
        }
    }
}

private struct MarkdownParagraphView: View {
    let runs: [FfiMdRun]
    let palette: UiPalette
    let strikeThrough: Bool

    var body: some View {
        Text(buildMarkdownAttributedString(
            runs: runs,
            palette: palette,
            baseSize: 13.5,
            strikeThrough: strikeThrough
        ))
        .foregroundStyle(Color(rgb: palette.foreground))
        .tint(Color(rgb: palette.accent))
        .lineSpacing(3)
        .textSelection(.enabled)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct MarkdownCodeBlockView: View {
    let lang: String?
    let code: String
    let palette: UiPalette

    @State private var copied = false
    @State private var resetTask: Task<Void, Never>?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Text((lang ?? "").lowercased())
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.dim))
                    .lineLimit(1)
                Spacer(minLength: 8)
                Button {
                    UIPasteboard.general.string = code
                    copied = true
                    resetTask?.cancel()
                    resetTask = Task { @MainActor in
                        try? await Task.sleep(for: .milliseconds(mdCopyFeedbackMs))
                        guard !Task.isCancelled else { return }
                        copied = false
                    }
                } label: {
                    Text(L.t(copied ? "chat_copied_code" : "chat_copy_code"))
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(Color(rgb: copied ? palette.accent : palette.dim))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(Color(rgb: palette.background), in: RoundedRectangle(cornerRadius: 4))
                        .overlay(
                            RoundedRectangle(cornerRadius: 4)
                                .stroke(Color(rgb: copied ? palette.accent : palette.line), lineWidth: 1)
                        )
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(Color(rgb: palette.panel2))

            Rectangle()
                .fill(Color(rgb: palette.line))
                .frame(height: 1)

            ScrollView(.horizontal, showsIndicators: true) {
                Text(code)
                    .font(.system(size: 12.5, design: .monospaced))
                    .foregroundStyle(Color(rgb: palette.foreground))
                    .lineSpacing(2.5)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: true, vertical: true)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 10)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(rgb: palette.panel))
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .overlay(
            RoundedRectangle(cornerRadius: 6)
                .stroke(Color(rgb: palette.line), lineWidth: 1)
        )
        .onChange(of: code) { _, _ in
            resetTask?.cancel()
            resetTask = nil
            copied = false
        }
        .onDisappear {
            resetTask?.cancel()
            resetTask = nil
        }
    }
}

private struct MarkdownBlockQuoteView: View {
    let doc: FfiMdDocument
    let children: [UInt32]
    let palette: UiPalette
    let depth: Int
    let listDepth: Int
    let strikeThrough: Bool

    @ViewBuilder
    var body: some View {
        if depth >= mdMaxDepth - 1 {
            MarkdownFlattenedBlockListView(
                doc: doc,
                indices: children,
                palette: palette,
                strikeThrough: strikeThrough
            )
        } else {
            VStack(alignment: .leading, spacing: 6) {
                MarkdownBlockListView(
                    doc: doc,
                    indices: children,
                    palette: palette,
                    depth: depth + 1,
                    listDepth: listDepth,
                    strikeThrough: strikeThrough
                )
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.leading, 12)
            .overlay(alignment: .leading) {
                Capsule()
                    .fill(Color(rgb: palette.accent))
                    .frame(width: 2)
            }
            .opacity(0.85)
        }
    }
}

private struct MarkdownListView: View {
    let doc: FfiMdDocument
    let ordered: Bool
    let start: UInt64
    let items: [FfiMdListItem]
    let palette: UiPalette
    let depth: Int
    let listDepth: Int
    let strikeThrough: Bool

    @ViewBuilder
    var body: some View {
        if depth >= mdMaxDepth - 1 {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    MarkdownFlattenedBlockListView(
                        doc: doc,
                        indices: item.children,
                        palette: palette,
                        strikeThrough: strikeThrough || (item.checked == true)
                    )
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        } else {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(items.enumerated()), id: \.offset) { idx, item in
                    let isCheckedTask = item.checked == true
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(marker(for: item, index: idx))
                            .font(.system(size: 13.5, design: .monospaced))
                            .foregroundStyle(Color(rgb: isCheckedTask ? palette.accent : palette.dim))
                        VStack(alignment: .leading, spacing: 4) {
                            itemChildren(item, isCheckedTask: isCheckedTask)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .opacity(isCheckedTask ? 0.72 : 1)
                    }
                }
            }
            .padding(.leading, listDepth == 0 ? 2 : 10)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private func itemChildren(_ item: FfiMdListItem, isCheckedTask: Bool) -> some View {
        if let firstIdx = item.children.first,
           doc.blocks.indices.contains(Int(firstIdx)),
           case .paragraph = doc.blocks[Int(firstIdx)] {
            MarkdownBlockView(
                doc: doc,
                block: doc.blocks[Int(firstIdx)],
                palette: palette,
                depth: depth + 1,
                listDepth: listDepth + 1,
                strikeThrough: strikeThrough || isCheckedTask
            )
            if item.children.count > 1 {
                MarkdownBlockListView(
                    doc: doc,
                    indices: Array(item.children.dropFirst()),
                    palette: palette,
                    depth: depth + 1,
                    listDepth: listDepth + 1,
                    strikeThrough: strikeThrough
                )
            }
        } else {
            MarkdownBlockListView(
                doc: doc,
                indices: item.children,
                palette: palette,
                depth: depth + 1,
                listDepth: listDepth + 1,
                strikeThrough: strikeThrough || isCheckedTask
            )
        }
    }

    private func marker(for item: FfiMdListItem, index: Int) -> String {
        if let checked = item.checked {
            return checked ? "[x]" : "[ ]"
        }
        if ordered {
            return "\(start + UInt64(index))."
        }
        switch listDepth % 3 {
        case 0: return "•"
        case 1: return "◦"
        default: return "▪"
        }
    }
}

private struct MarkdownTableView: View {
    let alignments: [FfiMdAlign]
    let headers: [FfiMdTableCell]
    let rows: [[FfiMdTableCell]]
    let palette: UiPalette

    private var colCount: Int {
        max(headers.count, rows.map(\.count).max() ?? 0, alignments.count)
    }

    var body: some View {
        let count = colCount
        if count > 0 {
            ScrollView(.horizontal, showsIndicators: true) {
                Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                    GridRow {
                        ForEach(0..<count, id: \.self) { col in
                            tableCell(
                                runs: col < headers.count ? headers[col].runs : [],
                                align: col < alignments.count ? alignments[col] : .none,
                                isHeader: true
                            )
                        }
                    }
                    ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                        GridRow {
                            ForEach(0..<count, id: \.self) { col in
                                tableCell(
                                    runs: col < row.count ? row[col].runs : [],
                                    align: col < alignments.count ? alignments[col] : .none,
                                    isHeader: false
                                )
                            }
                        }
                    }
                }
                .overlay(Rectangle().stroke(Color(rgb: palette.line), lineWidth: 1))
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func tableCell(runs: [FfiMdRun], align: FfiMdAlign, isHeader: Bool) -> some View {
        let attr = buildMarkdownAttributedString(runs: runs, palette: palette, baseSize: 12.5, baseBold: isHeader)
        let content = attr.characters.isEmpty ? AttributedString(" ") : attr
        return Text(content)
            .foregroundStyle(Color(rgb: isHeader ? palette.mdHeading : palette.foreground))
            .tint(Color(rgb: palette.accent))
            .multilineTextAlignment(textAlignment(for: align))
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .frame(minWidth: 48, maxWidth: 280, maxHeight: .infinity, alignment: frameAlignment(for: align))
            .background(isHeader ? Color(rgb: palette.panel) : Color.clear)
            .overlay(Rectangle().stroke(Color(rgb: palette.line), lineWidth: 0.5))
    }

    private func textAlignment(for align: FfiMdAlign) -> TextAlignment {
        switch align {
        case .center: .center
        case .right: .trailing
        case .left, .none: .leading
        }
    }

    private func frameAlignment(for align: FfiMdAlign) -> Alignment {
        switch align {
        case .center: .top
        case .right: .topTrailing
        case .left, .none: .topLeading
        }
    }
}
