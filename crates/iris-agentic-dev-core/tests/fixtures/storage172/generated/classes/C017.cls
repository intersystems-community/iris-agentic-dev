Class IadSm172Gen.C017 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Index IDKEY On K1 [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="F1">
<Delimiter>$C(1)</Delimiter>
<Node>"a"</Node>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Delimiter>$C(1)</Delimiter>
<Node>"b"</Node>
<Piece>1</Piece>
</Data>
<Global>^IadSm172Gen017</Global>
<Subscript name="1">
<Expression>{K1}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen017S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
