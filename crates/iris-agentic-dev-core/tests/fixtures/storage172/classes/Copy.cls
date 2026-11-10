Class IadSm172.Copy Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property BookId As %String;

Property Branch As %String;

Property CopyNo As %Integer;

Property Shelf As %String;

Index IDKEY On (BookId, Branch, CopyNo) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="Shelf">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Global>^IadSm172Copy</Global>
<RowIdSpec name="1">
<Expression>{L1}</Expression>
<Field>BookId</Field>
</RowIdSpec>
<RowIdSpec name="2">
<Expression>{L2}</Expression>
<Field>Branch</Field>
</RowIdSpec>
<RowIdSpec name="3">
<Expression>{L3}</Expression>
<Field>CopyNo</Field>
</RowIdSpec>
<Subscript name="1">
<Expression>{BookId}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{Branch}</Expression>
</Subscript>
<Subscript name="3">
<Expression>{CopyNo}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172CopyS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

