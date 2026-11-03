"""Generate deterministic, synthetic AcroForm and annotation fixtures."""

from __future__ import annotations

import json
from pathlib import Path

import pikepdf
from pikepdf import Array, Dictionary, Name, String

from .manifest import update_generated_hashes
from .pdf_tools import (
    _default_page_info,
    _new_pdf,
    _page_stream,
    _save_deterministic,
    _truth_envelope,
)


def _page(pdf: pikepdf.Pdf, resources: pikepdf.Object, title: str) -> pikepdf.Page:
    page = pdf.add_blank_page(page_size=(595.28, 841.89))
    page.obj[Name.Resources] = resources
    page.obj[Name.Contents] = pdf.make_stream(_page_stream([title]))
    page.obj[Name.Annots] = Array()
    return page


def _appearance(
    pdf: pikepdf.Pdf, width: float, height: float, on: bool = False
) -> pikepdf.Object:
    path = (
        f"q 1 g 0 0 {width} {height} re f 0 G 1 w 0.5 0.5 {width - 1} {height - 1} re S"
    )
    if on:
        path += f" 2 2 m {width - 2} {height - 2} l 2 {height - 2} m {width - 2} 2 l S"
    stream = pdf.make_stream((path + " Q\n").encode("ascii"))
    stream[Name.Type] = Name.XObject
    stream[Name.Subtype] = Name.Form
    stream[Name.BBox] = Array([0, 0, width, height])
    return stream


def _truth(
    stem: str,
    title: str,
    pages: list[dict],
    *,
    fields: list[dict] | None = None,
    annotations: list[dict] | None = None,
    javascript: dict | None = None,
    extra: dict | None = None,
) -> bytes:
    return _truth_envelope(
        doc_id=stem,
        title=title,
        page_count=len(pages),
        pages=pages,
        has_forms=bool(fields),
        field_names=[f["name"] for f in fields or []],
        form_fields=fields,
        has_annotations=bool(fields or annotations),
        annotations=annotations,
        has_javascript=bool(javascript),
        javascript=javascript,
        extra=extra,
    )


def _all_types(no_appearance: bool = False) -> tuple[bytes, bytes, list[dict]]:
    stem = "forms-no-appearance" if no_appearance else "forms-all-types"
    title = "Synthetic form field coverage"
    pdf, resources = _new_pdf(title)
    pages = [_page(pdf, resources, title), _page(pdf, resources, title)]
    font = resources[Name.Font][Name.F1]
    zapf = pdf.make_indirect(
        Dictionary(Type=Name.Font, Subtype=Name.Type1, BaseFont=Name.ZapfDingbats)
    )
    dr = Dictionary(Font=Dictionary(Helv=font, ZaDb=zapf))
    acro = pdf.make_indirect(
        Dictionary(Fields=Array(), DA=String("/Helv 11 Tf 0 g"), DR=dr)
    )
    if no_appearance:
        acro[Name.NeedAppearances] = True
    pdf.Root[Name.AcroForm] = acro
    fields: list[dict] = []

    def widget(
        name: str,
        kind: str,
        page: int,
        rect: list[int],
        *,
        flags: int = 0,
        value: str = "",
        options: list[str] | None = None,
        on: str = "On",
        max_length: int | None = None,
        align: int = 0,
    ) -> pikepdf.Object:
        ft = {
            "text": Name.Tx,
            "checkbox": Name.Btn,
            "radio": Name.Btn,
            "combo": Name.Ch,
            "list": Name.Ch,
            "button": Name.Btn,
            "signature": Name.Sig,
        }[kind]
        obj = Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            FT=ft,
            T=String(name),
            Rect=Array(rect),
            F=4,
            Ff=flags,
            P=pages[page - 1].obj,
        )
        if kind in ("text", "combo", "list"):
            obj[Name.V] = (
                Array() if kind == "list" and flags & (1 << 21) else String(value)
            )
            obj[Name.DV] = (
                Array() if kind == "list" and flags & (1 << 21) else String(value)
            )
            obj[Name.DA] = String("/Helv 11 Tf 0 g")
            obj[Name.Q] = align
        elif kind == "checkbox":
            obj[Name.V] = Name.Off
            obj[Name.AS] = Name.Off
        if options is not None:
            obj[Name.Opt] = Array([String(option) for option in options])
        if max_length is not None:
            obj[Name.MaxLen] = max_length
        if not no_appearance:
            width, height = rect[2] - rect[0], rect[3] - rect[1]
            off = _appearance(pdf, width, height)
            if kind == "checkbox":
                states = Dictionary(Off=off)
                states[Name("/" + on)] = _appearance(pdf, width, height, True)
                obj[Name.AP] = Dictionary(N=states)
            else:
                obj[Name.AP] = Dictionary(N=off)
        ref = pdf.make_indirect(obj)
        pages[page - 1].obj[Name.Annots].append(ref)
        acro[Name.Fields].append(ref)
        fields.append(
            {
                "name": name,
                "type": kind,
                "value": ([] if kind == "list" and flags & (1 << 21) else value)
                if kind in ("text", "combo", "list")
                else ("Off" if kind == "checkbox" else None),
                "default_value": ([] if kind == "list" and flags & (1 << 21) else value)
                if kind in ("text", "combo", "list")
                else None,
                "page": page,
                "rect": rect,
                "flags": flags,
                "options": options or [],
                "max_length": max_length,
                "alignment": align,
                "widgets": [
                    {
                        "page": page,
                        "rect": rect,
                        "on_state": on if kind == "checkbox" else None,
                    }
                ],
            }
        )
        return ref

    specs = [
        ("name", "text", 1, 0, None, None),
        ("multiline", "text", 1, 4096, None, None),
        ("comb", "text", 1, 1 << 24, None, 8),
        ("password", "text", 1, 1 << 13, None, None),
        ("aligned", "text", 1, 0, None, None),
        ("check", "checkbox", 1, 0, None, None),
        ("choice", "combo", 1, 1 << 17, ["a", "b", "c"], None),
        ("editable", "combo", 1, (1 << 17) | (1 << 18), ["a", "b"], None),
        ("list", "list", 2, 0, ["a", "b"], None),
        ("multi", "list", 2, 1 << 21, ["a", "b", "c"], None),
        ("push", "button", 2, 1 << 16, None, None),
        ("signature", "signature", 2, 0, None, None),
        ("readonly", "text", 2, 1, None, None),
        ("required", "text", 2, 2, None, None),
    ]
    for index, (name, kind, page, flags, options, max_length) in enumerate(specs):
        row = index if page == 1 else index - 8
        y = 745 - row * 38
        widget(
            name,
            kind,
            page,
            [200, y, 400 if kind != "checkbox" else 220, y + 22],
            flags=flags,
            options=options,
            max_length=max_length,
            align=2 if name == "aligned" else 0,
        )

    # A radio group is represented by one parent and nameless widget children.
    parent = pdf.make_indirect(
        Dictionary(FT=Name.Btn, T=String("radio"), Ff=1 << 15, V=Name.a, Kids=Array())
    )
    radio_widgets = []
    for index, state in enumerate(("a", "b", "c")):
        rect = [200 + index * 30, 430, 220 + index * 30, 450]
        child = Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            Parent=parent,
            Rect=Array(rect),
            F=4,
            P=pages[0].obj,
            AS=Name("/" + (state if index == 0 else "Off")),
        )
        if not no_appearance:
            states = Dictionary(Off=_appearance(pdf, 20, 20))
            states[Name("/" + state)] = _appearance(pdf, 20, 20, True)
            child[Name.AP] = Dictionary(N=states)
        ref = pdf.make_indirect(child)
        parent[Name.Kids].append(ref)
        pages[0].obj[Name.Annots].append(ref)
        radio_widgets.append({"page": 1, "rect": rect, "on_state": state})
    acro[Name.Fields].append(parent)
    fields.append(
        {
            "name": "radio",
            "type": "radio",
            "value": "a",
            "default_value": None,
            "page": 1,
            "rect": radio_widgets[0]["rect"],
            "flags": 1 << 15,
            "options": ["a", "b", "c"],
            "widgets": radio_widgets,
        }
    )

    # Hierarchical names and inherited FT, Ff and DA.
    person = pdf.make_indirect(
        Dictionary(
            T=String("person"),
            FT=Name.Tx,
            Ff=0,
            DA=String("/Helv 11 Tf 0 g"),
            Kids=Array(),
        )
    )
    address = pdf.make_indirect(
        Dictionary(T=String("address"), Parent=person, Kids=Array())
    )
    person[Name.Kids].append(address)
    for parent_obj, local, qualified, y in [
        (person, "name", "person.name", 350),
        (address, "city", "person.address.city", 320),
    ]:
        rect = [200, y, 400, y + 22]
        obj = Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            Parent=parent_obj,
            T=String(local),
            Rect=Array(rect),
            F=4,
            P=pages[0].obj,
            V=String(""),
        )
        if not no_appearance:
            obj[Name.AP] = Dictionary(N=_appearance(pdf, 200, 22))
        ref = pdf.make_indirect(obj)
        parent_obj[Name.Kids].append(ref)
        pages[0].obj[Name.Annots].append(ref)
        fields.append(
            {
                "name": qualified,
                "type": "text",
                "value": "",
                "default_value": None,
                "page": 1,
                "rect": rect,
                "flags": 0,
                "options": [],
                "widgets": [{"page": 1, "rect": rect, "on_state": None}],
            }
        )
    acro[Name.Fields].append(person)

    # One terminal field owns two widgets on different pages.
    shared = pdf.make_indirect(
        Dictionary(T=String("shared"), FT=Name.Tx, V=String(""), Kids=Array())
    )
    shared_widgets = []
    for page_no in (1, 2):
        rect = [200, 270, 400, 292]
        obj = Dictionary(
            Type=Name.Annot,
            Subtype=Name.Widget,
            Parent=shared,
            Rect=Array(rect),
            F=4,
            P=pages[page_no - 1].obj,
        )
        if not no_appearance:
            obj[Name.AP] = Dictionary(N=_appearance(pdf, 200, 22))
        ref = pdf.make_indirect(obj)
        shared[Name.Kids].append(ref)
        pages[page_no - 1].obj[Name.Annots].append(ref)
        shared_widgets.append({"page": page_no, "rect": rect, "on_state": None})
    acro[Name.Fields].append(shared)
    fields.append(
        {
            "name": "shared",
            "type": "text",
            "value": "",
            "default_value": None,
            "page": 1,
            "rect": shared_widgets[0]["rect"],
            "flags": 0,
            "options": [],
            "widgets": shared_widgets,
        }
    )
    truth = _truth(
        stem, title, [_default_page_info(1), _default_page_info(2)], fields=fields
    )
    return _save_deterministic(pdf), truth, fields


def _rotated() -> tuple[bytes, bytes]:
    stem, title = "forms-rotated", "Rotated form and annotation coverage"
    pdf, resources = _new_pdf(title)
    pages = [_page(pdf, resources, title), _page(pdf, resources, title)]
    acro = pdf.make_indirect(
        Dictionary(
            Fields=Array(),
            DA=String("/Helv 11 Tf 0 g"),
            DR=Dictionary(Font=Dictionary(Helv=resources[Name.Font][Name.F1])),
        )
    )
    pdf.Root[Name.AcroForm] = acro
    fields, annotations, page_truth = [], [], []
    for number, rotation in enumerate((90, 270), 1):
        page = pages[number - 1]
        page.obj[Name.Rotate] = rotation
        page.obj[Name.CropBox] = Array([20, 30, 575, 812])
        page_truth.append(_default_page_info(number, rotation=rotation))
        page_truth[-1]["crop_box"] = [20, 30, 575, 812]
        for kind, rect in (
            ("text", [100, 700, 300, 724]),
            ("checkbox", [100, 650, 120, 670]),
        ):
            name = f"{kind}_{number}"
            obj = Dictionary(
                Type=Name.Annot,
                Subtype=Name.Widget,
                T=String(name),
                FT=Name.Tx if kind == "text" else Name.Btn,
                Rect=Array(rect),
                F=4,
                P=page.obj,
            )
            if kind == "text":
                obj[Name.V] = String("")
                obj[Name.AP] = Dictionary(N=_appearance(pdf, 200, 24))
            else:
                obj[Name.V] = Name.Off
                obj[Name.AS] = Name.Off
                obj[Name.AP] = Dictionary(
                    N=Dictionary(
                        Off=_appearance(pdf, 20, 20), On=_appearance(pdf, 20, 20, True)
                    )
                )
            ref = pdf.make_indirect(obj)
            acro[Name.Fields].append(ref)
            page.obj[Name.Annots].append(ref)
            fields.append(
                {
                    "name": name,
                    "type": kind,
                    "value": "" if kind == "text" else "Off",
                    "page": number,
                    "rect": rect,
                    "widgets": [
                        {
                            "page": number,
                            "rect": rect,
                            "on_state": "On" if kind == "checkbox" else None,
                        }
                    ],
                }
            )
        for subtype, rect in (
            ("Highlight", [100, 600, 220, 620]),
            ("Text", [100, 550, 120, 570]),
        ):
            obj = Dictionary(
                Type=Name.Annot,
                Subtype=Name("/" + subtype),
                Rect=Array(rect),
                Contents=String(f"{subtype} {number}"),
                F=4,
                P=page.obj,
            )
            if subtype == "Highlight":
                obj[Name.QuadPoints] = Array([100, 620, 220, 620, 100, 600, 220, 600])
            ref = pdf.make_indirect(obj)
            page.obj[Name.Annots].append(ref)
            annotations.append(
                {
                    "subtype": subtype,
                    "page": number,
                    "rect": rect,
                    "contents": f"{subtype} {number}",
                    "has_appearance": False,
                }
            )
    return _save_deterministic(pdf), _truth(
        stem, title, page_truth, fields=fields, annotations=annotations
    )


def _variant(stem: str) -> tuple[bytes, bytes]:
    from io import BytesIO

    base, _, fields = _all_types()
    pdf = pikepdf.Pdf.open(BytesIO(base))
    title = "Synthetic form field coverage"
    pages = [_default_page_info(1), _default_page_info(2)]
    fields_by_name = {
        str(field.get(Name.T, "")): field for field in pdf.Root.AcroForm.Fields
    }
    extra = None
    javascript = None
    annotations = None
    if stem == "forms-xfa-stub":
        pdf.Root.AcroForm[Name.XFA] = Array(
            [
                String("template"),
                pdf.make_stream(
                    b"<template xmlns='http://www.xfa.org/schema/xfa-template/3.3/'/>"
                ),
            ]
        )
        extra = {"xfa": True}
    elif stem == "forms-signed":
        fields_by_name["signature"][Name.V] = pdf.make_indirect(
            Dictionary(
                Type=Name.Sig,
                ByteRange=Array([0, 1, 2, 1]),
                Contents=String(bytes(128)),
            )
        )
        for field in fields:
            if field["name"] == "signature":
                field["signed"] = True
        extra = {"signature_valid": False}
    elif stem == "forms-js-actions":
        locations = {}

        def action(number: int, location: str) -> pikepdf.Object:
            marker = f"JS_MARKER_{number}"
            locations[marker] = location
            return Dictionary(
                S=Name.JavaScript,
                JS=String(
                    f"/* {marker} */ this.getField('js_target').value=this.getField('js_source').value;"
                ),
            )

        source = fields_by_name["name"]
        source[Name.T] = String("js_source")
        target = fields_by_name["multiline"]
        target[Name.T] = String("js_target")
        for field in fields:
            if field["name"] == "name":
                field["name"] = "js_source"
            if field["name"] == "multiline":
                field["name"] = "js_target"
        source[Name.AA] = Dictionary(
            F=action(1, "field js_source /AA /F"),
            K=action(2, "field js_source /AA /K"),
            V=action(3, "field js_source /AA /V"),
        )
        target[Name.AA] = Dictionary(C=action(4, "field js_target /AA /C"))
        pdf.Root.AcroForm[Name.CO] = Array([target])
        fields_by_name["push"][Name.A] = action(5, "field push /A")
        page = pdf.pages[0].obj
        link = pdf.make_indirect(
            Dictionary(
                Type=Name.Annot,
                Subtype=Name.Link,
                Rect=Array([40, 40, 150, 60]),
                P=page,
                A=action(6, "link /A"),
            )
        )
        page.Annots.append(link)
        annotations = [
            {
                "subtype": "Link",
                "page": 1,
                "rect": [40, 40, 150, 60],
                "has_appearance": False,
            }
        ]
        page[Name.AA] = Dictionary(O=action(7, "page 1 /AA /O"))
        pdf.Root[Name.OpenAction] = action(8, "catalog /OpenAction")
        pdf.Root[Name.Names] = Dictionary(
            JavaScript=Dictionary(
                Names=Array([String("script"), action(9, "catalog /Names /JavaScript")])
            )
        )
        javascript = {"markers": locations, "must_never_execute": True}
    else:
        raise ValueError(stem)
    return _save_deterministic(pdf), _truth(
        stem,
        title,
        pages,
        fields=fields,
        annotations=annotations,
        javascript=javascript,
        extra=extra,
    )


def _annotations_all() -> tuple[bytes, bytes]:
    stem, title = "annotations-all-types", "Synthetic annotations coverage"
    pdf, resources = _new_pdf(title)
    page = _page(pdf, resources, title).obj
    types = [
        "Highlight",
        "Underline",
        "StrikeOut",
        "Squiggly",
        "Text",
        "FreeText",
        "Ink",
        "Line",
        "Line",
        "Square",
        "Circle",
        "Polygon",
        "PolyLine",
        "Stamp",
        "Link",
    ]
    annotations = []
    refs = []
    for index, subtype in enumerate(types):
        x = 40 + (index % 3) * 170
        y = 740 - (index // 3) * 75
        rect = [x, y, x + 120, y + 30]
        obj = Dictionary(
            Type=Name.Annot,
            Subtype=Name("/" + subtype),
            Rect=Array(rect),
            P=page,
            NM=String(f"annotation-{index}"),
            Contents=String(f"Annotation {index}"),
            F=4,
        )
        if subtype in ("Highlight", "Underline", "StrikeOut", "Squiggly"):
            obj[Name.QuadPoints] = Array([x, y + 30, x + 120, y + 30, x, y, x + 120, y])
        elif subtype in ("Line",):
            obj[Name.L] = Array([x, y, x + 120, y + 30])
            if index == 8:
                obj[Name.LE] = Array([Name("/None"), Name.OpenArrow])
        elif subtype in ("Polygon", "PolyLine"):
            obj[Name.Vertices] = Array([x, y, x + 60, y + 30, x + 120, y])
        elif subtype == "Ink":
            obj[Name.InkList] = Array([Array([x, y, x + 60, y + 30, x + 120, y])])
        elif subtype == "Stamp":
            obj[Name.Name] = Name.Approved
        elif subtype == "Link":
            obj[Name.A] = Dictionary(S=Name.URI, URI=String("https://example.org/"))
        if index % 2 == 0 and subtype != "Link":
            obj[Name.AP] = Dictionary(N=_appearance(pdf, 120, 30))
        if index == 1:
            obj[Name.F] = 2
        if index == 2:
            obj[Name.F] = 32
        ref = pdf.make_indirect(obj)
        page.Annots.append(ref)
        refs.append(ref)
        annotations.append(
            {
                "subtype": subtype,
                "page": 1,
                "rect": rect,
                "contents": f"Annotation {index}",
                "has_appearance": Name.AP in obj,
                "flags": int(obj.F),
                "uri": "https://example.org/" if subtype == "Link" else None,
                "ocg_off": index == 3,
            }
        )
    popup = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Popup,
            Rect=Array([50, 300, 180, 400]),
            Parent=refs[4],
        )
    )
    refs[4][Name.Popup] = popup
    page.Annots.append(popup)
    reply = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.Text,
            Rect=Array([60, 250, 80, 270]),
            IRT=refs[4],
            Contents=String("reply"),
        )
    )
    page.Annots.append(reply)
    payload = pdf.make_stream(b"synthetic attachment\n")
    payload[Name.Type] = Name.EmbeddedFile
    filespec = pdf.make_indirect(
        Dictionary(
            Type=Name.Filespec, F=String("attachment.txt"), EF=Dictionary(F=payload)
        )
    )
    attachment = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot,
            Subtype=Name.FileAttachment,
            Rect=Array([90, 250, 110, 270]),
            FS=filespec,
        )
    )
    page.Annots.append(attachment)
    redact = pdf.make_indirect(
        Dictionary(
            Type=Name.Annot, Subtype=Name.Redact, Rect=Array([120, 250, 150, 270])
        )
    )
    page.Annots.append(redact)
    layer = pdf.make_indirect(Dictionary(Type=Name.OCG, Name=String("Hidden layer")))
    pdf.Root[Name.OCProperties] = Dictionary(
        OCGs=Array([layer]), D=Dictionary(OFF=Array([layer]))
    )
    refs[3][Name.OC] = layer
    annotations.extend(
        [
            {
                "subtype": "Popup",
                "page": 1,
                "rect": [50, 300, 180, 400],
                "popup_of": "annotation-4",
                "has_appearance": False,
            },
            {
                "subtype": "Text",
                "page": 1,
                "rect": [60, 250, 80, 270],
                "reply_to": "annotation-4",
                "contents": "reply",
                "has_appearance": False,
            },
            {
                "subtype": "FileAttachment",
                "page": 1,
                "rect": [90, 250, 110, 270],
                "has_appearance": False,
            },
            {
                "subtype": "Redact",
                "page": 1,
                "rect": [120, 250, 150, 270],
                "has_appearance": False,
            },
        ]
    )
    return _save_deterministic(pdf), _truth(
        stem, title, [_default_page_info(1)], annotations=annotations
    )


def _cyclic() -> tuple[bytes, bytes]:
    stem, title = "forms-cyclic", "Synthetic cyclic PDF structures"
    pdf, resources = _new_pdf(title)
    page = _page(pdf, resources, title).obj
    field = pdf.make_indirect(Dictionary(FT=Name.Tx, T=String("cycle"), Kids=Array()))
    field[Name.Parent] = field
    field.Kids.append(field)
    pdf.Root[Name.AcroForm] = Dictionary(Fields=Array([field]))
    first = pdf.make_indirect(
        Dictionary(Type=Name.Annot, Subtype=Name.Text, Rect=Array([40, 40, 60, 60]))
    )
    second = pdf.make_indirect(
        Dictionary(Type=Name.Annot, Subtype=Name.Text, Rect=Array([70, 40, 90, 60]))
    )
    first[Name.IRT] = second
    second[Name.IRT] = first
    first[Name.Popup] = second
    second[Name.Popup] = first
    annots = pdf.make_indirect(Array([first, second]))
    annots.append(annots)
    page[Name.Annots] = annots
    fields = [{"name": "cycle", "type": "text", "value": None, "page": 1, "rect": None}]
    annotations = [
        {"subtype": "Text", "page": 1, "rect": [40, 40, 60, 60], "reply_to": "second"},
        {"subtype": "Text", "page": 1, "rect": [70, 40, 90, 60], "reply_to": "first"},
    ]
    return _save_deterministic(pdf), _truth(
        stem,
        title,
        [_default_page_info(1)],
        fields=fields,
        annotations=annotations,
        extra={"cycles": ["Kids", "Parent", "Annots", "IRT", "Popup"]},
    )


def _data_files() -> dict[str, bytes]:
    import csv
    from io import StringIO
    from xml.sax.saxutils import escape

    records = [
        {"name": "An Nguyễn", "check": "On", "choice": "a", "person.name": "An"},
        {"name": "Bình", "check": "Off", "choice": "b", "person.name": "Bình"},
        {"name": "Chi\nNguyễn", "check": "On", "choice": "c", "person.name": "Chi"},
        {"name": "Dũng", "check": "Off", "choice": "a", "person.name": "Dũng"},
        {"name": "Én", "check": "On", "choice": "b", "person.name": "Én"},
    ]
    single = records[0]
    prefix = "fixtures/pdf-forms/formdata/"
    json_single = (json.dumps(single, ensure_ascii=False, indent=2) + "\n").encode(
        "utf-8"
    )
    json_batch = (json.dumps(records, ensure_ascii=False, indent=2) + "\n").encode(
        "utf-8"
    )
    out = StringIO(newline="")
    writer = csv.DictWriter(out, fieldnames=list(single), lineterminator="\r\n")
    writer.writeheader()
    writer.writerows(records)
    fields_xml = "".join(
        f'<field name="{escape(k)}"><value>{escape(v)}</value></field>'
        for k, v in single.items()
        if "." not in k
    )
    fields_xml += f'<field name="person"><field name="name"><value>{escape(single["person.name"])}</value></field></field>'
    xfdf = (
        '<?xml version="1.0" encoding="UTF-8"?><xfdf xmlns="http://ns.adobe.com/xfdf/">'
        f'<f href="forms-all-types.pdf"/><fields>{fields_xml}</fields></xfdf>'
    ).encode()

    def utf16_hex(s: str) -> str:
        return "<FEFF" + s.encode("utf-16-be").hex().upper() + ">"

    fdf_fields = " ".join(
        f"<< /T {utf16_hex(k)} /V {utf16_hex(v)} >>"
        for k, v in single.items()
        if "." not in k
    )
    fdf_fields += f" << /T {utf16_hex('person')} /Kids [<< /T {utf16_hex('name')} /V {utf16_hex(single['person.name'])} >>] >>"
    fdf = (
        f"%FDF-1.2\n1 0 obj\n<< /FDF << /Fields [{fdf_fields}] >> >>\nendobj\n"
        "trailer << /Root 1 0 R >>\n%%EOF\n"
    ).encode("ascii")
    return {
        prefix + "forms-all-types.json": json_single,
        prefix + "forms-all-types.xfdf": xfdf,
        prefix + "forms-all-types.fdf": fdf,
        prefix + "batch.csv": out.getvalue().encode("utf-8"),
        prefix + "batch.json": json_batch,
        prefix
        + "hostile-doctype.xfdf": b'<!DOCTYPE xfdf [<!ENTITY x SYSTEM "file:///etc/passwd">]><xfdf xmlns="http://ns.adobe.com/xfdf/">&x;</xfdf>',
    }


def generate(root: Path) -> dict[str, bytes]:
    """Return generated PDF corpus, truth and form-data files."""
    del root
    outputs: dict[str, bytes] = {}
    for stem, builder in [
        ("forms-all-types", lambda: _all_types(False)[:2]),
        ("forms-no-appearance", lambda: _all_types(True)[:2]),
        ("forms-rotated", _rotated),
        ("forms-js-actions", lambda: _variant("forms-js-actions")),
        ("forms-xfa-stub", lambda: _variant("forms-xfa-stub")),
        ("forms-signed", lambda: _variant("forms-signed")),
        ("forms-cyclic", _cyclic),
        ("annotations-all-types", _annotations_all),
    ]:
        pdf_bytes, truth = builder()
        if len(pdf_bytes) >= 50_000:
            raise ValueError(f"fixture too large: {stem}")
        outputs[f"fixtures/pdf-forms/{stem}.pdf"] = pdf_bytes
        outputs[f"fixtures/pdf-forms/{stem}.truth.json"] = truth
    outputs.update(_data_files())
    if sum(map(len, outputs.values())) > 250_000:
        raise ValueError("form fixture corpus exceeds its budget")
    return outputs


def main() -> None:
    root = Path(__file__).resolve().parents[4]
    outputs = generate(root)
    for relative, content in sorted(outputs.items()):
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if not path.exists() or path.read_bytes() != content:
            path.write_bytes(content)
    update_generated_hashes(root, set(outputs))
    print(f"Generated {len(outputs)} form fixture files.")


if __name__ == "__main__":
    main()
