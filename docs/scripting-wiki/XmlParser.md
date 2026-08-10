# XmlParser

A minimal XML DOM parser (sb0t JSXmlParser). You can parse an XML string or build a tree by hand.

```javascript
var parser = new XmlParser();
```

## Properties

```javascript
parser.available;  // bool (parse succeeded / tree exists)
parser.root;       // root XmlNode
parser.xml;        // the last XML string
parser.nodeName;   // root node name
parser.nodeValue;  // root node text
parser.childNodes; // root children
parser.attributes; // root attributes
parser.parentNode; // always null (root)
```

## Methods

| Method | What it does |
|---|---|
| `create(rootName)` | Start a tree with a root node |
| `getNodesByName(name)` | Find all nodes with that name (recursive) |
| `load(xml)` | Parse an XML string (bool) |

## XmlNode

```javascript
node.nodeName;    // name
node.nodeValue;   // text content
node.attributes;  // XmlAttrs
node.childNodes;  // array of XmlNode
node.parentNode;  // parent node
node.appendChild(n);      // add child
node.removeChild(n);      // remove child
node.getNodesByName(name) // recursive search
```

## XmlAttrs

```javascript
attrs.length;
attrs.getValue(name);
attrs.setValue(name, value);
attrs.removeValue(name);
// also indexable: attrs["id"]
```

## Example: parse XML

```javascript
function onCommand(user, command, target, args) {
    if (command === "xml") {
        var p = new XmlParser();
        if (!p.load("<config><greet msg='hello'/><max value='10'/></config>")) {
            user.sendPM("bad xml");
            return;
        }
        var nodes = p.getNodesByName("greet");
        if (nodes.length > 0) {
            user.sendPM("greet msg: " + nodes[0].attributes.getValue("msg"));
        }
    }
}
```

## Example: build XML by hand

```javascript
var p = new XmlParser();
var root = p.create("room");
var child = new XmlNode("user");
child.attributes.setValue("name", "Alice");
root.appendChild(child);
print(root.getNodesByName("user").length); // 1
```
