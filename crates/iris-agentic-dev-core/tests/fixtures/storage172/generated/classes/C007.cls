Class IadSm172Gen.C007 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property K1 As %Integer;

Property K2 As %Integer;

Property F1 As %String(COLLATION = "EXACT");

Property F2 As %String;

Index IDKEY On (K1, K2) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="F1">
<Delimiter>$C(1)</Delimiter>
<Node>"d"</Node>
<Piece>1</Piece>
</Data>
<Data name="F2">
<Delimiter>$C(1)</Delimiter>
<Node>"d"</Node>
<Piece>2</Piece>
</Data>
<Global>^IadSm172Gen007</Global>
<Subscript name="1">
<AccessType>Sub</AccessType>
<Expression>{K1}</Expression>
</Subscript>
<Subscript name="2">
<AccessType>Sub</AccessType>
<Expression>{K2}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172Gen007S</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
